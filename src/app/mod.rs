//! The basecoin ABCI application.

pub(crate) mod modules;
mod response;
pub(crate) mod store;

use crate::app::modules::{prefix, Bank, Error, ErrorDetail, Ibc, Identifiable, Module};
use crate::app::response::ResponseFromErrorExt;
use crate::app::store::{ InMemoryStore,
    Height, Identifier, Path, ProvableStore, RevertibleStore, SharedStore, Store, SubStore,
};
use crate::prostgen::cosmos::auth::v1beta1::BaseAccount;
use crate::prostgen::cosmos::base::tendermint::v1beta1::{
    service_server::Service as HealthService, GetBlockByHeightRequest, GetBlockByHeightResponse,
    GetLatestBlockRequest, GetLatestBlockResponse, GetLatestValidatorSetRequest,
    GetLatestValidatorSetResponse, GetNodeInfoRequest, GetNodeInfoResponse, GetSyncingRequest,
    GetSyncingResponse, GetValidatorSetByHeightRequest, GetValidatorSetByHeightResponse,
    Module as VersionInfoModule, VersionInfo,
};
use crate::prostgen::cosmos::tx::v1beta1::service_server::Service as TxService;
use crate::prostgen::cosmos::tx::v1beta1::{
    BroadcastTxRequest, BroadcastTxResponse, GetTxRequest, GetTxResponse, GetTxsEventRequest,
    GetTxsEventResponse, SimulateRequest, SimulateResponse,
};

use std::convert::TryInto;
use std::sync::{Arc, RwLock};

use cosmrs::Tx;
use prost::Message;
use prost_types::Any;
use serde_json::Value;
use tendermint_abci::Application;
use tendermint_proto::abci::{
    Event, RequestBeginBlock, RequestDeliverTx, RequestInfo, RequestInitChain, RequestQuery,
    ResponseBeginBlock, ResponseCommit, ResponseDeliverTx, ResponseInfo, ResponseInitChain,
    ResponseQuery,
};
use tendermint_proto::types::Header as ProtoHeader;
use tendermint_proto::crypto::ProofOp;
use tendermint_proto::crypto::ProofOps;
use tendermint_proto::p2p::DefaultNodeInfo;
use tracing::{debug, info};
use module_service::module_service::module_server::{Module as ModuleService, ModuleServer};
use tonic::transport::Server;
use module_service::module_service::module_client::ModuleClient;
use tonic::{Request, Response, Status, Code};

use crate::app::module_service::module_service::{ModuleDeliverReply, ModuleRequest, ModuleReply, ModuleQuery, ModuleResponseQuery, ModuleBeginRequest};
use tendermint::block::Header;

pub mod module_service;
type MainStore<S> = SharedStore<RevertibleStore<S>>;
type ModuleStore<S> = SubStore<MainStore<S>>;
type Shared<T> = Arc<RwLock<T>>;


#[derive(Clone)]
pub(crate) struct ModuleServices<S> {
    pub modules: Shared<Vec<Box<dyn Module<Store = ModuleStore<S>> + Send + Sync>>>,
}

impl<S: Default + ProvableStore + 'static> ModuleServices<S> {
    /// Constructor.
    pub(crate) fn new(_modules: Shared<Vec<Box<dyn Module<Store = ModuleStore<S>> + Send + Sync>>>) -> Result<Self, S::Error> {
        Ok(Self {
            modules: _modules,/*Arc::new(RwLock::new(modules)),*/
        })
    }
}
#[tonic::async_trait]
impl<S: Default + ProvableStore + 'static> ModuleService for ModuleServices<S> {
        async fn init(
                    &self,
                    request: Request<ModuleRequest>,
        ) -> Result<Response<ModuleReply>, Status> {
          println!("Got a request: {:?}", request);
          let mut modules = self.modules.write().unwrap();
          let app_state: Value = serde_json::from_str(
            &String::from_utf8(request.get_ref().data.clone()).expect("invalid genesis state"),
          )
          .expect("genesis state isn't valid JSON");
          for m in modules.iter_mut() {
             m.init(app_state.clone());
          }


          let reply = crate::app::module_service::module_service::ModuleReply {
                   message: format!("Hello blockchain {:?}!", request.get_ref().data).into(),
                   };

        Ok(Response::new(reply))
     }

     async fn query(
                    &self,
                    request: Request<ModuleQuery>,
        ) -> Result<Response<ModuleResponseQuery>, Status> {
        println!("Got a request: {:?}", request);

        let path: Option<Path> = request.get_ref().path.clone().try_into().ok();
        let modules = self.modules.read().unwrap();
        for m in modules.iter() {
            match m.query(
                &request.get_ref().data,
                path.as_ref(),
                Height::from(request.get_ref().height as u64),
                request.get_ref().prove,
            ) {
                // success - implies query was handled by this module, so return response
                Ok(result) => {
                  let mut ops = vec![];
                  if let Some(mut proofs) = result.proof {
                    ops.append(&mut proofs);
                  };
                  let proofops = ProofOps {
                    ops
                  };
                  let reply = crate::app::module_service::module_service::ModuleResponseQuery {
                    data: result.data, 
                    proof_ops: Some(proofops)

                  };
                  return Ok(Response::new(reply));

                },
                Err(Error(ErrorDetail::NotHandled(_), _)) => continue,
                Err(e) => return Err(Status::new(Code::InvalidArgument, format!("query error: {:?}", e))),
            }
        }


            Err(Status::new(Code::InvalidArgument, "name is invalid"))
     }

    async fn deliver_msg(&self, message: Request<ModuleRequest>
        ) -> Result<Response<ModuleDeliverReply>, Status> {
        let mut modules = self.modules.write().unwrap();
        let mut handled = false;
        let mut events = vec![];

        let tx: Tx = match message.get_ref().data.as_slice().try_into() {
          Ok(tx) => tx,
          Err(err) => {
              return Err(Status::new(Code::InvalidArgument, "name is invalid"));
          },
        };

        if tx.body.messages.is_empty() {
            return Err(Status::new(Code::InvalidArgument,  "Empty Tx"));
        }
        for message in tx.body.messages {
            // try to deliver message to every module
            let _message = message.clone();
            for m in modules.iter_mut() {
              match m.deliver(message.clone()) {
                // success - append events and continue with next message
                Ok(mut msg_events) => {
                    events.append(&mut msg_events);
                    handled = true;
                    break;
                }
                Err(Error(ErrorDetail::NotHandled(_), _)) => continue,
                Err(e) => {
                    return Err(Status::new(Code::InvalidArgument, "name is invalid"));
                }
              }
            }
        }

        let reply = crate::app::module_service::module_service::ModuleDeliverReply {
                   events: events

                   };

        Ok(Response::new(reply))
    }
    async  fn begin_block(&self, message: Request<ModuleBeginRequest>)
        -> Result<Response<ModuleDeliverReply>, Status> {
        debug!("Got begin block request.");

        let mut modules = self.modules.write().unwrap();
        let mut events = vec![];
        let header = message.get_ref().header.as_ref().unwrap().clone().try_into().unwrap();
        for m in modules.iter_mut() {
            events.extend(m.begin_block(&header));
        }

        let reply = crate::app::module_service::module_service::ModuleDeliverReply {
                   events: events

        };

        Ok(Response::new(reply))
    }
    async fn commit(&self, message: Request<ModuleRequest>) 
        -> Result<Response<ModuleReply>, Status> {
        let mut modules = self.modules.write().unwrap();
        for m in modules.iter_mut() {
            m.store().commit().expect("failed to commit to state");
        }
        let reply = crate::app::module_service::module_service::ModuleReply {
           message: format!("Hello blockchain {:?}!", "3"),
        };

        Ok(Response::new(reply))

    }
}

#[tokio::main]
async fn serve<S: Default + ProvableStore + 'static>(store: Shared<Vec<Box<dyn Module<Store = ModuleStore<S>> + Send + Sync>>>) {
    let addr = "127.0.0.1:3000".parse().unwrap();

    let _module_services = ModuleServices::new(store).unwrap();
    Server::builder()
      .add_service(ModuleServer::new(_module_services))
      .serve(addr)
      .await.unwrap();

}

#[tokio::main]
async fn init_chain(_data: Vec<u8>) {
    let mut client = ModuleClient::connect("http://127.0.0.1:3000").await.unwrap();
    let request = tonic::Request::new(ModuleRequest {
        data: _data,
    });

    let response = client.init(request).await.unwrap();

    info!("Bank initialized {:?}", response);

}
#[tokio::main]
async fn query(_data: Vec<u8>, _path: String, _height: i64, _prove: bool) -> Result<Response<ModuleResponseQuery>, Status> {
    let mut client = ModuleClient::connect("http://127.0.0.1:3000").await.unwrap();
    let request = tonic::Request::new(ModuleQuery {
        data: _data,
        path: _path,
        height: _height,
        prove: _prove
    });

    let response = client.query(request).await;
    response


}

#[tokio::main]
async fn deliver_tx(_data: Vec<u8>) -> Result<Response<ModuleDeliverReply>, Status>{
    let mut client = ModuleClient::connect("http://127.0.0.1:3000").await.unwrap();
    let request = tonic::Request::new(ModuleRequest {
        data: _data,
    });

    let response = client.deliver_msg(request).await;
    response

}

#[tokio::main]
async fn begin_block(_header: ProtoHeader) -> Result<Response<ModuleDeliverReply>, Status>{
    let mut client = ModuleClient::connect("http://127.0.0.1:3000").await.unwrap();
    let request = tonic::Request::new(ModuleBeginRequest {
        header: Some(_header),
    });

    let response = client.begin_block(request).await;

    response
}

#[tokio::main]
async fn commit() -> Result<Response<ModuleReply>, Status> {
    let mut client = ModuleClient::connect("http://127.0.0.1:3000").await.unwrap();

    let _tx = vec![];
    let request = tonic::Request::new(ModuleRequest {
        data: _tx 
    });

    let response = client.commit(request).await;
    response

}

/// Unique identifiers for accounts.
pub type AccountId = String;

/// BaseCoin ABCI application.
///
/// Can be safely cloned and sent across threads, but not shared.
#[derive(Clone)]
pub(crate) struct BaseCoinApp<S> {
    pub store: MainStore<S>,
    pub modules: Shared<Vec<Box<dyn Module<Store = ModuleStore<S>> + Send + Sync>>>,
    account: Shared<BaseAccount>, // TODO(hu55a1n1): get from user and move to provable store
    remote_module: bool
}

impl<S: Default + ProvableStore + 'static> BaseCoinApp<S> {
    /// Constructor.
    pub(crate) fn new(store: S) -> Result<Self, S::Error> {
        let store = SharedStore::new(RevertibleStore::new(store));
        // `SubStore` guarantees modules exclusive access to all paths in the store key-space.
        let modules: Vec<Box<dyn Module<Store = ModuleStore<S>> + Send + Sync>> = vec![
            Box::new(Bank::new(SubStore::new(
                store.clone(),
                prefix::Bank {}.identifier(),
            )?)),
            Box::new(Ibc::new(SubStore::new(
                store.clone(),
                prefix::Ibc {}.identifier(),
            )?)),
        ];
        let _modules = Arc::new(RwLock::new(modules));
        let __modules = _modules.clone();
        std::thread::spawn(move || serve(__modules));
        Ok(Self {
            store,
            modules: _modules,
            account: Default::default(),
            remote_module: true,
        })
    }
}

impl<S: Default + ProvableStore> BaseCoinApp<S> {
    pub(crate) fn get_store(&self, prefix: Identifier) -> Option<ModuleStore<S>> {
        let mut modules = self.modules.write().unwrap();
        for m in modules.iter_mut() {
            if m.store().prefix() == prefix {
                return Some(m.store().clone());
            }
        }
        None
    }

}

impl<S: Default + ProvableStore + 'static> Application for BaseCoinApp<S> {
    fn info(&self, request: RequestInfo) -> ResponseInfo {
        let (last_block_height, last_block_app_hash) = {
            let state = self.store.read().unwrap();
            (state.current_height() as i64, state.root_hash())
        };
        debug!(
            "Got info request. Tendermint version: {}; Block version: {}; P2P version: {}, {:?}, {:?}",
            request.version, request.block_version, request.p2p_version, last_block_height, last_block_app_hash
        );
        ResponseInfo {
            data: "basecoin-rs".to_string(),
            version: "0.1.0".to_string(),
            app_version: 1,
            last_block_height,
            last_block_app_hash,
        }
    }

    fn init_chain(&self, request: RequestInitChain) -> ResponseInitChain {
        debug!("Got init chain request.");

        // safety - we panic on errors to prevent chain creation with invalid genesis config
        let app_state: Value = serde_json::from_str(
            &String::from_utf8(request.app_state_bytes.clone()).expect("invalid genesis state"),
        )
        .expect("genesis state isn't valid JSON");
        let data = request.app_state_bytes.clone();
        std::thread::spawn(move || init_chain(data));

        info!("App initialized");

        ResponseInitChain {
            consensus_params: request.consensus_params,
            validators: vec![], // use validator set proposed by tendermint (ie. in the genesis file)
            app_hash: self.store.write().unwrap().root_hash(),
        }
    }

    fn query(&self, request: RequestQuery) -> ResponseQuery {
        debug!("Got query request: {:?}", request);

        let path: Option<Path> = request.path.try_into().ok();
        let modules = self.modules.read().unwrap();
        let _data = request.data.clone();
        let _path = path.clone().unwrap().to_string();
        let _height = request.height;
        let _prove = request.prove;
        let result_data = std::thread::spawn(move || query(_data, 
                                         _path, 
                                        _height,
                                        _prove)).join();
          match result_data {
            Ok(result) => {
                    let store = self.store.read().unwrap();
                    let _result = result.unwrap();
                    let proof_ops = if request.prove {
                        let proof = store
                            .get_proof(
                                Height::from(request.height as u64),
                                &"ibc".to_owned().try_into().unwrap(),
                            )
                            .unwrap();
                        let mut buffer = Vec::new();
                        proof.encode(&mut buffer).unwrap(); // safety - cannot fail since buf is a vector
                        
                        let mut ops = vec![];
                        let  proofs = _result.get_ref().proof_ops.as_ref().unwrap().clone();
                       

                            for i in 0..proofs.ops.len() {
                              ops.push(proofs.ops[i].clone());
                            }
                        
                        ops.push(ProofOp {
                            r#type: "".to_string(),
                            // FIXME(hu55a1n1)
                            key: "ibc".to_string().into_bytes(),
                            data: buffer,
                        });
                        Some(ProofOps { ops })
                    } else {
                        None
                    };
                    return ResponseQuery {
                        code: 0,
                        log: "exists".to_string(),
                        key: request.data,
                        value: _result.get_ref().data.clone(),
                        proof_ops,
                        height: store.current_height() as i64,
                        ..Default::default()
                    };
            },
            Err(e) => return ResponseQuery::from_error(1, format!("query error: {:?}", e)),
          }
        ResponseQuery::from_error(1, "query msg not handled")
    }

    fn deliver_tx(&self, request: RequestDeliverTx) -> ResponseDeliverTx {
        debug!("Got deliverTx request: {:?}", request);
        let mut events = vec![];

          let _tx = request.tx.clone();
          let result = std::thread::spawn(move || deliver_tx(_tx)).join();


          let result_data = match result {
            Ok(res) => res.unwrap(),
            Err(err) => {
              return ResponseDeliverTx::from_error(
                  1,
                  format!("failed to deliver tx: {:?}", err),
                  );
            }
          };
          events.extend(result_data.get_ref().events.clone());
        

        ResponseDeliverTx {
            log: "success".to_owned(),
            events,
            ..ResponseDeliverTx::default()
        }
    }

    fn commit(&self) -> ResponseCommit {
        let mut modules = self.modules.write().unwrap();
        std::thread::spawn(move || commit());

        let mut state = self.store.write().unwrap();
        let data = state.commit().expect("failed to commit to state");
        info!(
            "Committed height {} with hash({})",
            state.current_height() - 1,
            data.iter()
                .map(|b| format!("{:02X}", b))
                .collect::<String>()
        );
        ResponseCommit {
            data,
            retain_height: 0,
        }
    }

    fn begin_block(&self, request: RequestBeginBlock) -> ResponseBeginBlock {
        debug!("Got begin block request.");

        let mut modules = self.modules.write().unwrap();
        let mut events = vec![];
        let _header: ProtoHeader = request.header.clone().unwrap().try_into().unwrap();
        std::thread::spawn(move || begin_block(_header));
        let header: Header = request.header.unwrap().try_into().unwrap();
        for m in modules.iter_mut() {
            events.extend(m.begin_block(&header));
        }

        ResponseBeginBlock { events }
    }
}

#[tonic::async_trait]
impl<S: ProvableStore + 'static> TxService for BaseCoinApp<S> {
    async fn simulate(
        &self,
        request: Request<SimulateRequest>,
    ) -> Result<Response<SimulateResponse>, Status> {
        // TODO(hu55a1n1): implement tx based simulate
        let _: Tx = request
            .into_inner()
            .tx_bytes
            .as_slice()
            .try_into()
            .map_err(|_| Status::invalid_argument("failed to deserialize tx"))?;
        Ok(Response::new(SimulateResponse {
            gas_info: None,
            result: None,
        }))
    }

    async fn get_tx(
        &self,
        _request: Request<GetTxRequest>,
    ) -> Result<Response<GetTxResponse>, Status> {
        unimplemented!()
    }

    async fn broadcast_tx(
        &self,
        _request: Request<BroadcastTxRequest>,
    ) -> Result<Response<BroadcastTxResponse>, Status> {
        unimplemented!()
    }

    async fn get_txs_event(
        &self,
        _request: Request<GetTxsEventRequest>,
    ) -> Result<Response<GetTxsEventResponse>, Status> {
        unimplemented!()
    }
}
