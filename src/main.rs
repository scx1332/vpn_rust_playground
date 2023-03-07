use actix::io::SinkWrite;
use actix::prelude::*;
use actix_codec::Framed;
use awc::ws::Frame;
use awc::{error::WsProtocolError, BoxedSocket};

use futures::stream::{SplitSink, SplitStream};
use futures_util::stream::StreamExt;
use log::{error, info};

use actix::Message;
use actix_web_actors::ws;
use actix_web_actors::ws::ProtocolError;
use bytes::Bytes;

/*
#[derive(Message)]
#[rtype(result = "WsResult<()>")]
pub struct Packet {
    pub data: Vec<u8>,
}

#[derive(Debug, Message)]
#[rtype(result = "WsResult<()>")]
pub struct Shutdown;
*/

type WsFramedSink = SplitSink<Framed<BoxedSocket, awc::ws::Codec>, awc::ws::Message>;
type WsFramedStream = SplitStream<Framed<BoxedSocket, awc::ws::Codec>>;

pub struct VpnWebSocket {
    //network_id: String,
    //heartbeat: Instant,
    //vpn: Recipient<Packet>,
    ws_sink: SinkWrite<ws::Message, WsFramedSink>,
}

impl VpnWebSocket {
    pub fn start(ws_sink: WsFramedSink, ws_stream: WsFramedStream) -> Addr<Self> {
        VpnWebSocket::create(|ctx| {
            ctx.add_stream(ws_stream);
            VpnWebSocket {
                ws_sink: SinkWrite::new(ws_sink, ctx),
            }
        })
    }

    fn forward(&self, _data: Vec<u8>, _ctx: &mut <Self as Actor>::Context) {
        println!("Forwarding data to VPN");
        /*
                let vpn = self.vpn.clone();
                vpn.send(Packet {
                    data,
                    meta: self.meta,
                })
                    .into_actor(self)
                    .map(move |result, this, ctx| {
                        if result.is_err() {
                            log::error!("VPN WebSocket: VPN {} no longer exists", this.network_id);
                            let _ = ctx.address().do_send(Shutdown {});
                        }
                    })
                    .wait(ctx);
        */
    }
}

impl Actor for VpnWebSocket {
    type Context = actix::Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        log::info!("VPN WebSocket: VPN connection started");
    }

    fn stopped(&mut self, _: &mut Self::Context) {
        log::info!("VPN WebSocket: VPN connection stopped");
    }
}

impl actix::io::WriteHandler<WsProtocolError> for VpnWebSocket {}

#[derive(Message, Debug)]
#[rtype(result = "()")]
struct Pong {
    msg: Bytes,
}

impl Handler<Pong> for VpnWebSocket {
    type Result = ();

    fn handle(&mut self, msg: Pong, _ctx: &mut Self::Context) {
        info!("Pushing Message {:?}", msg);
        if let Err(error) = self.ws_sink.write(ws::Message::Pong(msg.msg)) {
            error!("Error RosClient {:?}", error);
        }
    }
}

/*impl StreamHandler<Vec<u8>> for VpnWebSocket {
    fn handle(&mut self, data: Vec<u8>, ctx: &mut Self::Context) {
        ctx.binary(data)
    }
}*/

impl StreamHandler<Result<Frame, ProtocolError>> for VpnWebSocket {
    fn handle(&mut self, msg: Result<Frame, ProtocolError>, ctx: &mut Self::Context) {
        //self.heartbeat = Instant::now();
        match msg {
            Ok(Frame::Text(_text)) => {
                log::error!("VPN WebSocket: Received text frame");
                ctx.stop();
            }
            Ok(Frame::Binary(bytes)) => self.forward(bytes.to_vec(), ctx),
            Ok(Frame::Ping(msg)) => {
                log::info!("Received Ping Message, replying with pong...");
                if let Err(err) = self.ws_sink.write(ws::Message::Pong(msg)) {
                    log::error!("Error replying with pong: {:?}", err);
                    ctx.stop();
                }
            }
            Ok(Frame::Pong(_)) => {
                log::info!("Received Pong Message");
            }
            Ok(Frame::Close(reason)) => {
                //ctx.close(reason);
                log::info!("Received Close Message: {:?}", reason);
                ctx.stop();
            }
            Ok(Frame::Continuation(_)) => {
                //ignore
            }
            Err(err) => {
                log::error!("VPN WebSocket: protocol error: {:?}", err);
                ctx.stop();
            }
        }
    }
}

/*
impl Handler<Shutdown> for VpnWebSocket {
    type Result = <Shutdown as Message>::Result;

    fn handle(&mut self, _: Shutdown, ctx: &mut Self::Context) -> Self::Result {
        log::warn!("VPN WebSocket: VPN is shutting down");
        ctx.stop();
        Ok(())
    }
}*/

#[cfg(all(unix))]
async fn read_tun_interface(receive_bytes: std::sync::mpsc::Receiver<bytes::Bytes>) {
    use packet::ip::Packet;
    use std::sync::Arc;
    use std::sync::Mutex;
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncWriteExt;
    use tun::TunPacket;
    let mut config = tun::Configuration::default();
    config
        .address((10, 0, 0, 1))
        .netmask((255, 255, 255, 0))
        .up();

    //    let dev = Arc::new(Mutex::new(tun::create_as_async(&config).unwrap()));

    let dev = tun::create_as_async(&config).unwrap();

    //let (mut stream_stream) = reader.into_framed();
    let (mut stream_sink, mut stream_stream) =
        futures_util::stream::StreamExt::split(dev.into_framed());
    if false {
        actix::spawn(async move {
            println!("Waiting for stream...");
        });
    }

    println!("Waiting for packet...");

    while let Some(packet) = stream_stream.next().await {
        match packet {
            Ok(pkt) => {
                let b = bytes::Bytes::from(pkt.get_bytes().to_vec()).clone();
                println!("Sending packet: {:#?}", Packet::unchecked(b.clone()));
                // sink.send(awc::ws::Message::Binary(b))
                //   .await
                // .unwrap();
            }
            Err(err) => panic!("Error: {:?}", err),
        }
    }

    /*  let (mut read,mut  write) = tokio::io::split(dev);
    //    let dev_clone = dev.clone();


        loop {
            let mut buf = vec![0u8; 10];
            println!("Waiting for packet...");
            let bytes_read = read.read(&mut buf).await.unwrap();
            println!("Received packet: {:#?}", Packet::unchecked(bytes::Bytes::from(buf.clone())));
            if bytes_read > 0 {


    //            println!("Send packet: {:#?}", Packet::unchecked(bytes::Bytes::from(buf.clone())));

            //    sink.send(awc::ws::Message::Binary(bytes::Bytes::from(buf.clone())))
              //      .await
                //    .unwrap();
            }
        }*/
}

#[actix_rt::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::env::set_var(
        "RUST_LOG",
        std::env::var("RUST_LOG").unwrap_or("info".to_string()),
    );
    env_logger::init();
    let (_tx, _rx) = std::sync::mpsc::channel::<bytes::Bytes>();
    //let connector = awc::Connector::new().ssl(ssl).finish();
    let (_req, ws_socket) = awc::Client::default()
        .ws("ws://host.docker.internal:7465/net-api/v2/vpn/net/37b06a7a460346ebbc0ecd2ac14d812a/raw/192.168.8.7/50671")
        .header("Authorization", "Bearer 04233840468323139872")
        .connect()
        .await?;

    let (ws_sink, ws_stream) = ws_socket.split();
    let _addr = VpnWebSocket::start(ws_sink, ws_stream);
    //let addr = addr.start();

    //#[cfg(all(unix))]
    //read_tun_interface(sink, stream, rx).await;
    let _ = actix_rt::signal::ctrl_c().await?;
    Ok(())
}
