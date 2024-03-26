use std::fmt::Display;

pub mod compat {
    use std::fmt::Display;

    #[derive(Debug)]
    pub(crate) enum CompatibilityHyperError {
        HyperError(hyper::Error),
        LegacyHyperError(hyper_util::client::legacy::Error),
        HttpHyperError(hyper::http::Error),
        Infallible(std::convert::Infallible),
    }

    impl std::error::Error for CompatibilityHyperError {}

    impl Display for CompatibilityHyperError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                CompatibilityHyperError::HyperError(e) => write!(f, "HyperError: {}", e),
                CompatibilityHyperError::LegacyHyperError(e) => {
                    write!(f, "LegacyHyperError: {}", e)
                }
                CompatibilityHyperError::HttpHyperError(e) => write!(f, "HttpHyperError: {}", e),
                CompatibilityHyperError::Infallible(e) => write!(f, "Infallible: {}", e),
            }
        }
    }

    impl From<hyper::Error> for CompatibilityHyperError {
        fn from(e: hyper::Error) -> Self {
            CompatibilityHyperError::HyperError(e)
        }
    }

    impl From<hyper_util::client::legacy::Error> for CompatibilityHyperError {
        fn from(e: hyper_util::client::legacy::Error) -> Self {
            CompatibilityHyperError::LegacyHyperError(e)
        }
    }

    impl From<hyper::http::Error> for CompatibilityHyperError {
        fn from(e: hyper::http::Error) -> Self {
            CompatibilityHyperError::HttpHyperError(e)
        }
    }

    impl From<std::convert::Infallible> for CompatibilityHyperError {
        fn from(e: std::convert::Infallible) -> Self {
            CompatibilityHyperError::Infallible(e)
        }
    }
}

#[derive(Debug)]
pub enum ApoxyError {
    BrokenChannel,
}

impl Display for ApoxyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApoxyError::BrokenChannel => write!(f, "Broken channel"),
        }
    }
}

impl std::error::Error for ApoxyError {}
