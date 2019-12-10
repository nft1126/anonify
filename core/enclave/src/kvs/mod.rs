use std::prelude::v1::*;
use elastic_array::{ElasticArray128, ElasticArray32};
use ed25519_dalek::{PublicKey, Signature};
use crate::{
    error::Result,
    crypto::UserAddress,
};

mod memorydb;
mod traits;

pub use memorydb::{MemoryKVS, MEMORY_DB};
pub use traits::SigVerificationKVS;

/// Database value.
pub type DBValue = ElasticArray128<u8>;

/// Database operation
#[derive(Clone, PartialEq)]
pub enum DBOp {
    Insert {
        key: ElasticArray32<u8>,
        value: DBValue,
    },
    Delete {
        key: ElasticArray32<u8>,
    }
}

impl DBOp {
    /// Returns the key associated with this operation.
    pub fn key(&self) -> &[u8] {
        match *self {
            DBOp::Insert { ref key, .. } => key,
            DBOp::Delete { ref key, .. } => key,
        }
    }
}

/// Batches a sequence of put/delete operations for efficiency.
/// These operations are protected from signature verifications.
#[derive(Default, Clone, PartialEq)]
pub struct DBTx(InnerDBTx);

impl DBTx {
    pub fn new() -> Self {
        DBTx(InnerDBTx::new())
    }

    /// Put instruction is added to a transaction only if the verification of provided signature returns true.
    pub fn put(
        &mut self,
        pubkey: &PublicKey,
        sig: &Signature,
        msg: &[u8],
    ) {
        let key = UserAddress::from_sig(&msg, &sig, &pubkey);
        self.0.put(key.as_slice(), msg);
    }

    /// Delete instruction is added to a transaction only if the verification of provided signature returns true.
    pub fn delete(
        &mut self,
        msg: &[u8],
        sig: &Signature,
        pubkey: &PublicKey,
    ) {
        let key = UserAddress::from_sig(&msg, &sig, &pubkey);
        self.0.delete(key.as_slice());
    }

    pub(crate) fn into_inner(self) -> InnerDBTx {
        self.0
    }
}

/// Inner struct to write transaction. Batches a sequence of put/delete operations for efficiency.
#[derive(Default, Clone, PartialEq)]
pub(crate) struct InnerDBTx {
    /// Database operations.
    ops: Vec<DBOp>,
}

impl InnerDBTx {
    fn new() -> Self {
        Self::with_capacity(256)
    }

    fn with_capacity(cap: usize) -> Self {
        InnerDBTx {
            ops: Vec::with_capacity(cap)
        }
    }

    fn put(&mut self, key: &[u8], value: &[u8]) {
        let mut ekey = ElasticArray32::new();
        ekey.append_slice(key);
        self.ops.push(DBOp::Insert {
            key: ekey,
            value: DBValue::from_slice(value),
        });
    }

    fn delete(&mut self, key: &[u8]) {
        let mut ekey = ElasticArray32::new();
        ekey.append_slice(key);
        self.ops.push(DBOp::Delete {
            key: ekey,
        });
    }
}
