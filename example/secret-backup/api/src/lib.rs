use serde::{Deserialize, Serialize};

pub mod start {
    pub mod post {
        use super::super::*;

        #[derive(Debug, Clone, Eq, PartialEq, Hash, Default, Deserialize, Serialize)]
        pub struct Response {
            pub status: String,
        }
    }
}

pub mod stop {
    pub mod post {
        use super::super::*;

        #[derive(Debug, Clone, Eq, PartialEq, Hash, Default, Deserialize, Serialize)]
        pub struct Response(pub String);
    }
}
