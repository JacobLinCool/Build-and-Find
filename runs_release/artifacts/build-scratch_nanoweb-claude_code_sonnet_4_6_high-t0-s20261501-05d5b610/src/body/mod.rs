pub mod json;
pub mod multipart;
pub mod urlencoded;

use serde::de::DeserializeOwned;
use std::collections::HashMap;

use crate::error::NanowebError;

#[derive(Debug, Clone)]
pub struct FormField {
    pub name: String,
    pub value: FieldValue,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone)]
pub enum FieldValue {
    Text(String),
    File {
        filename: Option<String>,
        data: Vec<u8>,
    },
}

#[derive(Debug, Default, Clone)]
pub struct MultipartForm {
    pub fields: Vec<FormField>,
}

impl MultipartForm {
    pub fn get_text(&self, name: &str) -> Option<&str> {
        self.fields.iter().find_map(|f| {
            if f.name == name {
                if let FieldValue::Text(ref s) = f.value {
                    return Some(s.as_str());
                }
            }
            None
        })
    }

    pub fn get_file(&self, name: &str) -> Option<(&Option<String>, &[u8])> {
        self.fields.iter().find_map(|f| {
            if f.name == name {
                if let FieldValue::File {
                    ref filename,
                    ref data,
                } = f.value
                {
                    return Some((filename, data.as_slice()));
                }
            }
            None
        })
    }

    pub fn into_text_map(self) -> HashMap<String, String> {
        self.fields
            .into_iter()
            .filter_map(|f| {
                if let FieldValue::Text(v) = f.value {
                    Some((f.name, v))
                } else {
                    None
                }
            })
            .collect()
    }
}

pub fn parse_body_json<T: DeserializeOwned>(body: &[u8]) -> Result<T, NanowebError> {
    serde_json::from_slice(body).map_err(NanowebError::Json)
}

pub fn parse_body_form<T: DeserializeOwned>(body: &[u8]) -> Result<T, NanowebError> {
    serde_urlencoded::from_bytes(body).map_err(|e| NanowebError::Parse(e.to_string()))
}

pub fn parse_body_multipart(body: &[u8], boundary: &str) -> Result<MultipartForm, NanowebError> {
    multipart::parse(body, boundary)
}
