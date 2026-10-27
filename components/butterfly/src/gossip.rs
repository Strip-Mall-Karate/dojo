// Copyright (c) 2026 Strip Mall Karate Company
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! The gossip transport: rumors travel over TCP, one connection per sender and
//! gossip round, each rumor a frame of a 4-byte big-endian length followed by
//! its `Wire` message.

use std::{io::{self,
               Write},
          time::Duration};

use tokio_util::codec::LengthDelimitedCodec;

/// The largest frame a member accepts. Service files and configuration arrive
/// through the control gateway, whose body length is 20 bits (1 MiB), so a
/// rumor carrying one stays well under this.
pub const MAX_FRAME_LEN: usize = 8 * 1024 * 1024;

/// How long a sender may take to connect, or to write one frame.
pub const SEND_TIMEOUT: Duration = Duration::from_millis(500);

/// How long a receiver waits for a sender's next frame before it closes the
/// connection.
pub const RECV_TIMEOUT: Duration = Duration::from_secs(10);

/// The codec both ends of a gossip connection frame rumors with.
pub fn codec() -> LengthDelimitedCodec {
    LengthDelimitedCodec::builder().length_field_type::<u32>()
                                   .big_endian()
                                   .max_frame_length(MAX_FRAME_LEN)
                                   .new_codec()
}

/// Writes one frame to a blocking stream, for callers without a runtime.
pub fn write_frame<W: Write>(writer: &mut W, payload: &[u8]) -> io::Result<()> {
    if payload.len() > MAX_FRAME_LEN {
        return Err(io::Error::new(io::ErrorKind::InvalidInput,
                                  format!("a {}-byte rumor exceeds the {}-byte frame limit",
                                          payload.len(),
                                          MAX_FRAME_LEN)));
    }
    writer.write_all(&(payload.len() as u32).to_be_bytes())?;
    writer.write_all(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;
    use tokio_util::codec::Decoder;

    #[test]
    fn a_written_frame_decodes_with_the_codec() {
        let mut buf = Vec::new();
        write_frame(&mut buf, b"first").unwrap();
        write_frame(&mut buf, b"").unwrap();
        write_frame(&mut buf, b"third").unwrap();
        let mut buf = BytesMut::from(&buf[..]);
        let mut codec = codec();
        assert_eq!(&codec.decode(&mut buf).unwrap().unwrap()[..], b"first");
        assert_eq!(&codec.decode(&mut buf).unwrap().unwrap()[..], b"");
        assert_eq!(&codec.decode(&mut buf).unwrap().unwrap()[..], b"third");
        assert!(codec.decode(&mut buf).unwrap().is_none());
    }

    #[test]
    fn the_codec_refuses_a_frame_over_the_limit() {
        let mut buf = BytesMut::new();
        buf.extend_from_slice(&((MAX_FRAME_LEN + 1) as u32).to_be_bytes());
        assert!(codec().decode(&mut buf).is_err());
    }

    #[test]
    fn write_frame_refuses_a_payload_over_the_limit() {
        let mut buf = Vec::new();
        assert!(write_frame(&mut buf, &vec![0; MAX_FRAME_LEN + 1]).is_err());
        assert!(buf.is_empty());
    }
}
