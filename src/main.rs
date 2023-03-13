mod tap_codec;

use crate::tap_codec::{TapPacket, TapPacketCodec};
use actix::io::SinkWrite;
use actix::prelude::*;
use actix_codec::Framed;
use awc::ws;
use awc::{error::WsProtocolError, BoxedSocket};
use bytes::Bytes;
use etherparse::IpHeader::{Version4, Version6};
use etherparse::{PacketBuilder, TransportHeader};
use futures::stream::{SplitSink, SplitStream};
use futures_util::stream::StreamExt;
use std::net::IpAddr;
use structopt::StructOpt;
use tun::{AsyncDevice, TunPacket, TunPacketCodec};

type WsFramedSink = SplitSink<Framed<BoxedSocket, ws::Codec>, ws::Message>;
type WsFramedStream = SplitStream<Framed<BoxedSocket, ws::Codec>>;
type TunFramedSink = SplitSink<tokio_util::codec::Framed<AsyncDevice, TunPacketCodec>, TunPacket>;
type TunFramedStream = SplitStream<tokio_util::codec::Framed<AsyncDevice, TunPacketCodec>>;
type TapFramedSink = SplitSink<tokio_util::codec::Framed<AsyncDevice, TapPacketCodec>, TapPacket>;
type TapFramedStream = SplitStream<tokio_util::codec::Framed<AsyncDevice, TapPacketCodec>>;

pub struct VpnWebSocket {
    ws_sink: SinkWrite<ws::Message, WsFramedSink>,
    tap_sink: Option<SinkWrite<TapPacket, TapFramedSink>>,
    tun_sink: Option<SinkWrite<TunPacket, TunFramedSink>>,
}

impl VpnWebSocket {
    pub fn start(
        ws_sink: WsFramedSink,
        ws_stream: WsFramedStream,
        tun_sink: Option<TunFramedSink>,
        tun_stream: Option<TunFramedStream>,
        tap_sink: Option<TapFramedSink>,
        tap_stream: Option<TapFramedStream>,
    ) -> Addr<Self> {
        VpnWebSocket::create(|ctx| {
            ctx.add_stream(ws_stream);
            if tap_sink.is_some() && tun_sink.is_some() {
                panic!("Pass TUN or TAP arguments, but not both");
            }

            if let (Some(tap_sink), Some(tap_stream)) = (tap_sink, tap_stream) {
                ctx.add_stream(tap_stream);
                VpnWebSocket {
                    ws_sink: SinkWrite::new(ws_sink, ctx),
                    tun_sink: None,
                    tap_sink: Some(SinkWrite::new(tap_sink, ctx)),
                }
            } else if let (Some(tun_sink), Some(tun_stream)) = (tun_sink, tun_stream) {
                ctx.add_stream(tun_stream);
                VpnWebSocket {
                    ws_sink: SinkWrite::new(ws_sink, ctx),
                    tun_sink: Some(SinkWrite::new(tun_sink, ctx)),
                    tap_sink: None,
                }
            } else {
                panic!("Pass TUN or TAP arguments");
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
                if let Some(tun_sink) = self.tun_sink.as_mut() {
                    log::info!("Received Binary packet, sending to TUN...");

                    if let Ok(packet) = etherparse::PacketHeaders::from_ethernet_slice(&bytes) {
                        match &packet.ip {
                            None => {
                                log::error!("No IP header found");
                            }
                            Some(Version4(ip, _ipv4_extensions)) => {
                                match packet.transport {
                                    Some(TransportHeader::Udp(udp_header)) => {
                                        let builder = PacketBuilder::ipv4(
                                            ip.destination,
                                            ip.source,
                                            ip.time_to_live,
                                        )
                                        .udp(udp_header.destination_port, udp_header.source_port); //desitnation port
                                                                                                   // payload of the udp packet
                                        let payload = packet.payload;
                                        // get some memory to store the serialized data
                                        let mut complete_packet =
                                            Vec::<u8>::with_capacity(builder.size(payload.len()));
                                        builder.write(&mut complete_packet, &payload).unwrap();

                                        log::info!("Sending packet: {:?}", complete_packet);
                                        if let Err(err) =
                                            tun_sink.write(TunPacket::new(complete_packet))
                                        {
                                            log::error!("Error sending packet to TUN: {:?}", err);
                                            ctx.stop();
                                        }
                                        log::info!("udp: {:?}", udp_header);
                                    }
                                    Some(TransportHeader::Tcp(tcp_header)) => {
                                        log::info!("tcp: {:?}", tcp_header);
                                    }
                                    Some(TransportHeader::Icmpv4(icmp_header)) => {
                                        log::info!("icmp: {:?}", icmp_header);
                                    }
                                    Some(TransportHeader::Icmpv6(icmp_header)) => {
                                        log::info!("icmp: {:?}", icmp_header);
                                    }
                                    None => {
                                        log::info!("No transport header");
                                    }
                                }
                            }
                            Some(Version6(_ipv6_header, _ipv6_ext)) => {
                                log::info!("IpV6");
                            }
                        }
                    }
                } else {
                    log::info!("Received Binary packet, sending to TAP...");
                    if let Err(err) = self
                        .tap_sink
                        .as_mut()
                        .expect("tap sink has to be here")
                        .write(TapPacket::new(bytes.to_vec()))
                    {
                        log::error!("Error sending packet to TUN: {:?}", err);
                        ctx.stop();
                    }
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
                if let Ok(packet) = etherparse::PacketHeaders::from_ip_slice(&packet.get_bytes()) {
                    match &packet.ip {
                        None => {
                            log::error!("No IP header found");
                        }
                        Some(Version4(ip, _ipv4_extensions)) => {
                            match packet.transport {
                                Some(TransportHeader::Udp(udp_header)) => {
                                    let builder = PacketBuilder::ethernet2(
                                        [1, 1, 1, 1, 1, 1],
                                        [2, 2, 2, 2, 2, 2],
                                    )
                                    .ipv4(ip.destination, ip.source, ip.time_to_live)
                                    .udp(udp_header.destination_port, udp_header.source_port); //desitnation port
                                                                                               // payload of the udp packet
                                    let payload = packet.payload;
                                    // get some memory to store the serialized data
                                    let mut complete_packet =
                                        Vec::<u8>::with_capacity(builder.size(payload.len()));
                                    builder.write(&mut complete_packet, &payload).unwrap();

                                    log::info!("Sending wrapped eth packet: {:?}", complete_packet);
                                    if let Err(err) = self
                                        .ws_sink
                                        .write(ws::Message::Binary(Bytes::from(complete_packet)))
                                    {
                                        log::error!("Error sending packet to websocket: {:?}", err);
                                        ctx.stop();
                                    }
                                    log::info!("udp: {:?}", udp_header);
                                }
                                Some(TransportHeader::Tcp(tcp_header)) => {
                                    log::info!("tcp: {:?}", tcp_header);
                                }
                                Some(TransportHeader::Icmpv4(icmp_header)) => {
                                    log::info!("icmp: {:?}", icmp_header);
                                }
                                Some(TransportHeader::Icmpv6(icmp_header)) => {
                                    log::info!("icmp: {:?}", icmp_header);
                                }
                                None => {
                                    log::info!("No transport header");
                                }
                            }
                        }
                        Some(Version6(_ipv6_header, _ipv6_ext)) => {
                            log::info!("IpV6");
                        }
                    }
                }
                log::info!(
                    "Received packet from TUN {:#?}",
                    packet::ip::Packet::unchecked(packet.get_bytes())
                );
            }
            Err(err) => {
                log::error!("Tun io error: {:?}", err);
                ctx.stop();
            }
        }
    }
}

impl StreamHandler<Result<TapPacket, std::io::Error>> for VpnWebSocket {
    fn handle(&mut self, msg: Result<TapPacket, std::io::Error>, ctx: &mut Self::Context) {
        //self.heartbeat = Instant::now();
        match msg {
            Ok(packet) => {
                log::info!("Received packet from TUN {:#?}", packet.get_bytes());
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

#[derive(Debug, StructOpt, Clone)]
pub struct CliOptions {
    #[structopt(long = "http", help = "Enable http server")]
    pub http: bool,

    #[structopt(
        long = "http-threads",
        help = "Number of threads to use for the server",
        default_value = "2"
    )]
    pub http_threads: u64,

    #[structopt(
        long = "http-port",
        help = "Port number of the server",
        default_value = "8080"
    )]
    pub http_port: u16,

    #[structopt(
        long = "websocket-address",
        help = "Bind websocket address",
        default_value = "ws://host.docker.internal:7465/net-api/v2/vpn/net/dd45782a49374df98c9f6b94fd26702f/raw/from/192.168.8.1/to/192.168.8.7"
    )]
    pub websocket_address: String,

    #[structopt(
        long = "vpn-network-addr",
        help = "Bind address to the vpn network",
        default_value = "192.168.8.1"
    )]
    pub vpn_network_addr: String,

    #[structopt(
        long = "vpn-network-mast",
        help = "Vpn network mask",
        default_value = "255.255.255.0"
    )]
    pub vpn_network_mask: String,

    #[structopt(
        long = "vpn-interface-name",
        help = "Name of the vpn interface",
        default_value = "vpn0"
    )]
    pub vpn_interface_name: String,

    #[structopt(
    long = "vpn-layer",
    help = "Name of the vpn interface",
    default_value = "tun",
    possible_values = &["tun", "tap"]
    )]
    pub vpn_layer: String,
}

#[actix_rt::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::env::set_var(
        "RUST_LOG",
        std::env::var("RUST_LOG").unwrap_or("info".to_string()),
    );
    env_logger::init();
    let opt: CliOptions = CliOptions::from_args();
    let app_key = std::env::var("YAGNA_APPKEY").expect("YAGNA_APPKEY not set");
    let (_tx, _rx) = std::sync::mpsc::channel::<bytes::Bytes>();
    //let connector = awc::Connector::new().ssl(ssl).finish();
    let (_req, ws_socket) = awc::Client::default()
        .ws(opt.websocket_address)
        .header("Authorization", format!("Bearer {app_key}"))
        .connect()
        .await?;

    let (ws_sink, ws_stream) = ws_socket.split();

    let addr = opt.vpn_network_addr.parse::<IpAddr>()?;
    let mask = opt.vpn_network_mask.parse::<IpAddr>()?;
    let mut config = tun::Configuration::default();
    let vpn_layer = match opt.vpn_layer.as_str() {
        "tun" => tun::Layer::L3,
        "tap" => tun::Layer::L2,
        _ => panic!("Invalid vpn layer"),
    };
    config
        .layer(vpn_layer)
        .address(addr)
        .netmask(mask)
        .name(opt.vpn_interface_name)
        .up();

    let dev = tun::create_as_async(&config).unwrap();

    let _ws_actor = if opt.vpn_layer == "tap" {
        let (tap_sink, tap_stream) =
            tokio_util::codec::Framed::new(dev, TapPacketCodec::new()).split();
        VpnWebSocket::start(
            ws_sink,
            ws_stream,
            None,
            None,
            Some(tap_sink),
            Some(tap_stream),
        )
    } else {
        let (tun_sink, tun_stream) = dev.into_framed().split();
        VpnWebSocket::start(
            ws_sink,
            ws_stream,
            Some(tun_sink),
            Some(tun_stream),
            None,
            None,
        )
    };

    actix_rt::signal::ctrl_c().await?;
    Ok(())
}
