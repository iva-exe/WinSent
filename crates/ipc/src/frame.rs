//! Rámcování: každá zpráva = u32 LE prefix délky + postcard payload.
//! Prefix je zdroj pravdy o hranici zprávy — pipe běží v byte módu,
//! takže framing nezávisí na chování ReadFile u message pipes.

use std::io::{Read, Write};

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::{Error, MAX_FRAME_LEN};

/// Zapíše jednu zprávu jako rámec.
pub fn write_msg<W: Write, T: Serialize>(w: &mut W, msg: &T) -> Result<(), Error> {
    let payload = postcard::to_stdvec(msg)?;
    let len = payload.len() as u32;
    if len > MAX_FRAME_LEN {
        return Err(Error::FrameTooLarge {
            len,
            max: MAX_FRAME_LEN,
        });
    }
    w.write_all(&len.to_le_bytes())?;
    w.write_all(&payload)?;
    w.flush()?;
    Ok(())
}

/// Přečte jednu zprávu z rámce. `Ok(None)` = protistrana čistě zavřela
/// spojení (EOF na hranici rámce).
pub fn read_msg<R: Read, T: DeserializeOwned>(r: &mut R) -> Result<Option<T>, Error> {
    let mut len_buf = [0u8; 4];
    // EOF před prvním bajtem prefixu je korektní konec spojení.
    match r.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => return Ok(None),
        Err(e) => return Err(e.into()),
    }

    let len = u32::from_le_bytes(len_buf);
    if len > MAX_FRAME_LEN {
        return Err(Error::FrameTooLarge {
            len,
            max: MAX_FRAME_LEN,
        });
    }

    let mut payload = vec![0u8; len as usize];
    r.read_exact(&mut payload)?;
    Ok(Some(postcard::from_bytes(&payload)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_types::ipc::{Request, PROTOCOL_VERSION};

    // Round-trip přes paměťový buffer — rámec se musí přečíst zpět beze změny.
    #[test]
    fn frame_roundtrip() {
        let msg = Request::Ping {
            protocol_version: PROTOCOL_VERSION,
        };
        let mut buf = Vec::new();
        write_msg(&mut buf, &msg).unwrap();
        let back: Request = read_msg(&mut buf.as_slice()).unwrap().unwrap();
        assert_eq!(back, msg);
    }

    // Podvržený prefix délky nesmí vést k alokaci — pipe je útočná plocha.
    #[test]
    fn oversized_frame_is_rejected() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&u32::MAX.to_le_bytes());
        let res: Result<Option<Request>, _> = read_msg(&mut buf.as_slice());
        assert!(matches!(res, Err(Error::FrameTooLarge { .. })));
    }
}
