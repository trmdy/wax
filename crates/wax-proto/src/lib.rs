use std::fmt;

use serde::{Deserialize, Serialize};

pub const PROTO_VERSION: u32 = 0;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Unsupported,
    BadZip,
    TooLarge,
    Timeout,
    Internal,
}

impl ErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unsupported => "unsupported",
            Self::BadZip => "bad_zip",
            Self::TooLarge => "too_large",
            Self::Timeout => "timeout",
            Self::Internal => "internal",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_match_protocol_strings() {
        let codes = [
            (ErrorCode::Unsupported, "unsupported"),
            (ErrorCode::BadZip, "bad_zip"),
            (ErrorCode::TooLarge, "too_large"),
            (ErrorCode::Timeout, "timeout"),
            (ErrorCode::Internal, "internal"),
        ];

        for (code, expected) in codes {
            assert_eq!(code.as_str(), expected);
            assert_eq!(
                serde_json::to_string(&code).expect("code should serialize"),
                format!("\"{expected}\"")
            );
        }
    }
}
