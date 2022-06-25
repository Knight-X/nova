
use serde_json::Value;
use std::convert::TryInto;
use std::num::ParseIntError;
use std::str::FromStr;
use flex_error::{define_error, TraceError};
use std::collections::HashMap;
use tendermint_proto::abci::Event;
use prost_types::Any;
use crate::app::store::{Height, Path, Store};
use crate::app::modules::{Error as ModuleError, Module, QueryResult};
use prost::{DecodeError, Message};
use serde::{Deserialize, Serialize};
use crate::offchain::nn::run_dl;
pub type AccountId = String;
define_error! {
    #[derive(Eq, PartialEq)]
    Error {
        MsgDecodeFailure
            [ TraceError<DecodeError> ]
            | _ | { "failed to decode message" },
        MsgValidationFailure
            { reason: String }
            | e | { format!("failed to validate message: {}", e.reason) },
        NonExistentAccount
            { account: AccountId }
            | e | { format!("account {} doesn't exist", e.account) },
        InvalidAmount
            [ TraceError<ParseIntError> ]
            | _ | { "invalid amount specified" },
        InsufficientSourceFunds
            | _ | { "insufficient funds in sender account" },
        DestFundOverflow
            | _ | { "receiver account funds overflow" },
        Store
            { reason: String }
            | e | { format!("failed to validate message: {}", e.reason) },
    }
}

impl From<Error> for ModuleError {
    fn from(e: Error) -> Self {
        ModuleError::dnn(e)
    }
}
pub type Denom = String;
/// A mapping of currency denomination identifiers to balances.
#[derive(Serialize, Deserialize, Debug, Default)]
#[serde(transparent)]
pub struct NetworkWeights(Value);

#[derive(Clone)]
pub struct DnnStorage<S> {
    store: S,
}

impl<S: Store> DnnStorage<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }

    fn decode<T: Message + Default>(message: Any) -> Result<T, ModuleError> {
        if message.type_url != "/cosmos.bank.v1beta1.MsgSend" {
            return Err(ModuleError::not_handled());
        }
        Message::decode(message.value.as_ref()).map_err(|e| Error::msg_decode_failure(e).into())
    }
}

impl<S: Store> Module for DnnStorage<S> {
    type Store = S;

    fn deliver(&mut self, message: Any) -> Result<Vec<Event>, ModuleError> {
        std::thread::spawn(move || run_dl());
         Ok(vec![])
    }

    fn init(&mut self, app_state: serde_json::Value) {
        unimplemented!();
    }

    fn query(
        &self,
        data: &[u8],
        _path: Option<&Path>,
        height: Height,
        _prove: bool,
    ) -> Result<QueryResult, ModuleError> {
        unimplemented!();
    }

    fn store(&mut self) -> &mut S {
        &mut self.store
    }
}
