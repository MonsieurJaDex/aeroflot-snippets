use std::{error::Error, fmt};

#[derive(Debug)]
pub enum CommonErrors {
    InvalidArgument(String),
    IncorrectLayer(String),
    IncorrectPath(String),
    IncorrectFileContent(String),
}

impl fmt::Display for CommonErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(format!("{:?}", self).as_str())
    }
}

impl std::error::Error for CommonErrors {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        None
    }

    fn cause(&self) -> Option<&dyn Error> {
        self.source()
    }
}
