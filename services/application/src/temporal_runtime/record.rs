//! Record decoding before the SDK's generic JSON-to-Value normalization.
use serde::{Serialize, de::DeserializeOwned};
use temporalio_common::{
    data_converters::{PayloadConversionError, SerializationContext, TemporalDeserializable},
    protos::{ENCODING_PAYLOAD_KEY, JSON_ENCODING_VAL, temporal::api::common::v1::Payload},
};

/// Transparent Temporal record payload with direct object-only JSON decoding.
///
/// The default SDK composite converter emits the original inner JSON representation.
/// Its wrapper decoder consumes original bytes before generic Value normalization can
/// discard duplicate fields. Domain validation still belongs to the record's owner.
/// This intentionally has no Serde Deserialize implementation: that would select the
/// SDK blanket decoder instead of the raw payload boundary below.
#[derive(Clone, Serialize)]
#[serde(transparent)]
pub struct RecordPayload<T>(pub T);

impl<T: DeserializeOwned> TemporalDeserializable for RecordPayload<T> {
    fn from_payload(
        _: &SerializationContext<'_>,
        payload: Payload,
    ) -> Result<Self, PayloadConversionError> {
        if payload
            .metadata
            .get(ENCODING_PAYLOAD_KEY)
            .map(Vec::as_slice)
            != Some(JSON_ENCODING_VAL.as_bytes())
        {
            return Err(PayloadConversionError::WrongEncoding);
        }
        serde_json::from_slice::<crate::provider_json::Object<T>>(&payload.data)
            .map(|record| Self(record.0))
            .map_err(|error| PayloadConversionError::EncodingError(Box::new(error)))
    }
}
