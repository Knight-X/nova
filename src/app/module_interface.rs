
use crate::app::store::{
    Height, Path, ProvableStore, Store,
};
use tracing::{debug, info};
use crate::app::modules::{Error, ErrorDetail,  Module};
use tonic::{Request, Response, Status, Code};

use crate::app::module_service::module_service::module_server::{Module as ModuleService, ModuleServer};
use crate::app::module_service::module_service::{ModuleDeliverReply, ModuleRequest, ModuleReply, ModuleQuery, ModuleResponseQuery, ModuleBeginRequest};
use crate::app::{ModuleStore, Shared};
use std::convert::TryInto;
use cosmrs::Tx;
use serde_json::Value;
use tonic::transport::Server;
use crate::app::module_service::module_service::module_client::ModuleClient;
use tendermint_proto::crypto::ProofOps;
use tendermint_proto::types::Header as ProtoHeader;

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
pub(crate) async fn serve<S: Default + ProvableStore + 'static>(store: Shared<Vec<Box<dyn Module<Store = ModuleStore<S>> + Send + Sync>>>) {
    let addr = "127.0.0.1:3000".parse().unwrap();

    let _module_services = ModuleServices::new(store).unwrap();
    Server::builder()
      .add_service(ModuleServer::new(_module_services))
      .serve(addr)
      .await.unwrap();

}

#[tokio::main]
pub(crate) async fn init_chain(_data: Vec<u8>) {
    let mut client = ModuleClient::connect("http://127.0.0.1:3000").await.unwrap();
    let request = tonic::Request::new(ModuleRequest {
        data: _data,
    });

    let response = client.init(request).await.unwrap();

    info!("Bank initialized {:?}", response);

}
#[tokio::main]
pub async fn query(_data: Vec<u8>, _path: String, _height: i64, _prove: bool) -> Result<Response<ModuleResponseQuery>, Status> {
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
pub async fn deliver_tx(_data: Vec<u8>) -> Result<Response<ModuleDeliverReply>, Status>{
    let mut client = ModuleClient::connect("http://127.0.0.1:3000").await.unwrap();
    let request = tonic::Request::new(ModuleRequest {
        data: _data,
    });

    let response = client.deliver_msg(request).await;
    response

}

#[tokio::main]
pub async fn begin_block(_header: ProtoHeader) -> Result<Response<ModuleDeliverReply>, Status>{
    let mut client = ModuleClient::connect("http://127.0.0.1:3000").await.unwrap();
    let request = tonic::Request::new(ModuleBeginRequest {
        header: Some(_header),
    });

    let response = client.begin_block(request).await;

    response
}

#[tokio::main]
pub async fn commit() -> Result<Response<ModuleReply>, Status> {
    let mut client = ModuleClient::connect("http://127.0.0.1:3000").await.unwrap();

    let _tx = vec![];
    let request = tonic::Request::new(ModuleRequest {
        data: _tx 
    });

    let response = client.commit(request).await;
    response

}
