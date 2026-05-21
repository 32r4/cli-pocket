use crate::frame::Frame;
use crate::relay::{RelayCtrl, RelayData, RELAY_DISC_CTRL, RELAY_DISC_DATA};

#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("postcard: {0}")]
    Postcard(#[from] postcard::Error),
    #[error("empty frame")]
    Empty,
    #[error("unknown discriminator {0:#x}")]
    UnknownDiscriminator(u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayWire {
    Ctrl(RelayCtrl),
    Data(RelayData),
}

pub fn encode_frame(frame: &Frame) -> Result<Vec<u8>, CodecError> {
    Ok(postcard::to_allocvec(frame)?)
}

pub fn decode_frame(bytes: &[u8]) -> Result<Frame, CodecError> {
    Ok(postcard::from_bytes(bytes)?)
}

pub fn encode_relay_ctrl(ctrl: &RelayCtrl) -> Result<Vec<u8>, CodecError> {
    let mut out = Vec::with_capacity(1);
    out.push(RELAY_DISC_CTRL);
    out.extend_from_slice(&postcard::to_allocvec(ctrl)?);
    Ok(out)
}

pub fn encode_relay_data(data: &RelayData) -> Result<Vec<u8>, CodecError> {
    let mut out = Vec::with_capacity(1);
    out.push(RELAY_DISC_DATA);
    out.extend_from_slice(&postcard::to_allocvec(data)?);
    Ok(out)
}

pub fn decode_relay(bytes: &[u8]) -> Result<RelayWire, CodecError> {
    let (disc, rest) = bytes.split_first().ok_or(CodecError::Empty)?;
    match *disc {
        RELAY_DISC_CTRL => Ok(RelayWire::Ctrl(postcard::from_bytes(rest)?)),
        RELAY_DISC_DATA => Ok(RelayWire::Data(postcard::from_bytes(rest)?)),
        other => Err(CodecError::UnknownDiscriminator(other)),
    }
}
