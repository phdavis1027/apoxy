#[derive(Debug, Display)]
pub(crate) enum CompatibilityHyperError {
    HyperError(hyper::Error),
    LegacyHyperError(hyper_util::client::legacy::Error),
    HttpHyperError(hyper::http::Error),
    Infallible(std::convert::Infallible),
}

impl std::error::Error for CompatibilityHyperError {
    fn cause(&self) -> Option<&dyn std::error::Error> {
        match self {
            CompatibilityHyperError::HyperError(e) => Some(e),
            CompatibilityHyperError::LegacyHyperError(e) => Some(e),
            CompatibilityHyperError::HttpHyperError(e) => Some(e),
        }
    }

    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CompatibilityHyperError::HyperError(e) => Some(e),
            CompatibilityHyperError::LegacyHyperError(e) => Some(e),
            CompatibilityHyperError::HttpHyperError(e) => Some(e),
        }
    }
}
