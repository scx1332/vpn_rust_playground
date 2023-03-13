//            DO WHAT THE FUCK YOU WANT TO PUBLIC LICENSE
//                    Version 2, December 2004
//
// Copyleft (ↄ) meh. <meh@schizofreni.co> | http://meh.schizofreni.co
//
// Everyone is permitted to copy and distribute verbatim or modified
// copies of this license document, and changing it is allowed as long
// as the name is changed.
//
//            DO WHAT THE FUCK YOU WANT TO PUBLIC LICENSE
//   TERMS AND CONDITIONS FOR COPYING, DISTRIBUTION AND MODIFICATION
//
//  0. You just DO WHAT THE FUCK YOU WANT TO.

use std::io;

use bytes::{BufMut, Bytes, BytesMut};
use tokio_util::codec::{Decoder, Encoder};


/// A Tun Packet to be sent or received on the TUN interface.
#[derive(Debug)]
pub struct TapPacket(Bytes);

impl TapPacket {
    /// Create a new `TunPacket` based on a byte slice.
    pub fn new(bytes: Vec<u8>) -> TapPacket {
        //let proto = infer_proto(&bytes);
        TapPacket(Bytes::from(bytes))
    }

    /// Return this packet's bytes.
    pub fn get_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn into_bytes(self) -> Bytes {
        self.0
    }
}

/// A TunPacket Encoder/Decoder.
pub struct TapPacketCodec();

impl TapPacketCodec {
    /// Create a new `TapPacketCodec` specifying whether the underlying
    ///  tunnel Device has enabled the packet information header.
    pub fn new() -> TapPacketCodec {
        TapPacketCodec()
    }
}

impl Decoder for TapPacketCodec {
    type Item = TapPacket;
    type Error = io::Error;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if buf.is_empty() {
            return Ok(None);
        }

        let pkt = buf.split_to(buf.len());

        // reserve enough space for the next packet
        buf.reserve(90000);

        // if the packet information is enabled we have to ignore the first 4 bytes
        /*if self.0 {
            let _ = pkt.split_to(4);
        }*/

      //  let proto = infer_proto(pkt.as_ref());
        Ok(Some(TapPacket(pkt.freeze())))
    }
}

impl Encoder<TapPacket> for TapPacketCodec {
    type Error = io::Error;

    fn encode(&mut self, item: TapPacket, dst: &mut BytesMut) -> Result<(), Self::Error> {
        dst.reserve(item.get_bytes().len() + 4);
        match item {
            /*TapPacket(proto, bytes) if self.0 => {
                // build the packet information header comprising of 2 u16
                // fields: flags and protocol.
                let mut buf = Vec::<u8>::with_capacity(4);

                // flags is always 0
                buf.write_u16::<NativeEndian>(0).unwrap();
                // write the protocol as network byte order
                buf.write_u16::<NetworkEndian>(proto.into_pi_field()?)
                    .unwrap();

                dst.put_slice(&buf);
                dst.put(bytes);
            }*/
            TapPacket(bytes) => dst.put(bytes),
        }
        Ok(())
    }
}
