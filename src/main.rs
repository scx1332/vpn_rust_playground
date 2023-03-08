use actix::io::SinkWrite;
use actix::prelude::*;
use actix_codec::Framed;
use awc::ws;
use awc::{error::WsProtocolError, BoxedSocket};
use bytes::Bytes;
use futures::stream::{SplitSink, SplitStream};
use futures_util::stream::StreamExt;
use tun::AsyncDevice;
use tun::TunPacket;
use tun::TunPacketCodec;

type WsFramedSink = SplitSink<Framed<BoxedSocket, ws::Codec>, ws::Message>;
type WsFramedStream = SplitStream<Framed<BoxedSocket, ws::Codec>>;
type TunFramedSink = SplitSink<tokio_util::codec::Framed<AsyncDevice, TunPacketCodec>, TunPacket>;
type TunFramedStream = SplitStream<tokio_util::codec::Framed<AsyncDevice, TunPacketCodec>>;

pub struct VpnWebSocket {
    ws_sink: SinkWrite<ws::Message, WsFramedSink>,
    tun_sink: SinkWrite<TunPacket, TunFramedSink>,
}

impl VpnWebSocket {
    pub fn start(
        ws_sink: WsFramedSink,
        ws_stream: WsFramedStream,
        tun_sink: TunFramedSink,
        tun_stream: TunFramedStream,
    ) -> Addr<Self> {
        VpnWebSocket::create(|ctx| {
            ctx.add_stream(ws_stream);
            ctx.add_stream(tun_stream);
            VpnWebSocket {
                ws_sink: SinkWrite::new(ws_sink, ctx),
                tun_sink: SinkWrite::new(tun_sink, ctx),
            }
        })
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

impl io::WriteHandler<WsProtocolError> for VpnWebSocket {}
impl io::WriteHandler<std::io::Error> for VpnWebSocket {}

impl StreamHandler<Result<ws::Frame, WsProtocolError>> for VpnWebSocket {
    fn handle(&mut self, msg: Result<ws::Frame, WsProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Frame::Text(_text)) => {
                log::error!("VPN WebSocket: Received text frame");
                ctx.stop();
            }
            Ok(ws::Frame::Binary(bytes)) => {
                log::info!("Received Binary packet, sending to TUN...");
                if let Err(err) = self.tun_sink.write(TunPacket::new(bytes.to_vec())) {
                    log::error!("Error sending packet to TUN: {:?}", err);
                    ctx.stop();
                }
            }
            Ok(ws::Frame::Ping(msg)) => {
                log::info!("Received Ping Message, replying with pong...");
                if let Err(err) = self.ws_sink.write(ws::Message::Pong(msg)) {
                    log::error!("Error replying with pong: {:?}", err);
                    ctx.stop();
                }
            }
            Ok(ws::Frame::Pong(_)) => {
                log::info!("Received Pong Message");
            }
            Ok(ws::Frame::Close(reason)) => {
                //ctx.close(reason);
                log::info!("Received Close Message: {:?}", reason);
                ctx.stop();
            }
            Ok(ws::Frame::Continuation(_)) => {
                //ignore
            }
            Err(err) => {
                log::error!("VPN WebSocket: protocol error: {:?}", err);
                ctx.stop();
            }
        }
    }
}

impl StreamHandler<Result<TunPacket, std::io::Error>> for VpnWebSocket {
    fn handle(&mut self, msg: Result<TunPacket, std::io::Error>, ctx: &mut Self::Context) {
        //self.heartbeat = Instant::now();
        match msg {
            Ok(packet) => {
                log::info!(
                    "Received packet from TUN {:#?}",
                    packet::ip::Packet::unchecked(packet.get_bytes())
                );
                if let Err(err) = self.ws_sink.write(ws::Message::Binary(Bytes::from(
                    packet.get_bytes().to_vec(),
                ))) {
                    log::error!("Error sending packet to websocket: {:?}", err);
                    ctx.stop();
                }
            }
            Err(err) => {
                log::error!("Tun io error: {:?}", err);
                ctx.stop();
            }
        }
    }
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

    let mut config = tun::Configuration::default();
    config
        .address((10, 0, 0, 1))
        .netmask((255, 255, 255, 0))
        .up();

    let dev = tun::create_as_async(&config).unwrap();

    let (tun_sink, tun_stream) = dev.into_framed().split();
    let _ws_actor = VpnWebSocket::start(ws_sink, ws_stream, tun_sink, tun_stream);

    let _ = actix_rt::signal::ctrl_c().await?;
    Ok(())
}
