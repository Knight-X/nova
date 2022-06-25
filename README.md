# Nova
A rust-version cosmos-sdk fork from basecoin-rs 
* `bank` - keeps track of different accounts' balances and facilitates transactions between those accounts.
* `ibc` - enables support for IBC (clients, connections & channels)
* `dnn` - deep neural network module for training 

## Requirements
So far this app has been tested with:
* Rust >v1.52.1
* Tendermint v0.34.10

## Sepeficiation
  1. rust version cosmos-sdk for rusters
  2. offchain module for heavy computation  ex. dnn
  3. try to build golang compatible module from rust
  
## Reference for main.rs
 ```shell
 fn main() {
    let opt: Opt = Opt::from_args();
    let log_level = if opt.quiet {
        LevelFilter::OFF
    } else if opt.verbose {
        LevelFilter::TRACE
    } else {
        LevelFilter::INFO
    };
    tracing_subscriber::fmt().with_max_level(log_level).init();

    tracing::info!("Starting app and waiting for Tendermint to connect...");

    let app = BaseCoinApp::new(InMemoryStore::default()).expect("Failed to init app");
    let app_copy = app.clone();
    let grpc_port = opt.grpc_port;
    let grpc_host = opt.host.clone();
    std::thread::spawn(move || grpc_serve(app_copy, grpc_host, grpc_port));

    let server = ServerBuilder::new(opt.read_buf_size)
        .bind(format!("{}:{}", opt.host, opt.port), app)
        .unwrap();
    server.listen().unwrap();
}
```

## Run your Node
### Step 1: Reset your local Tendermint node
```shell
$ tendermint init
$ tendermint unsafe-reset-all
```

### Step 2: Modify Tendermint config
Edit the Tendermint `config.toml` file (default location `~/.tendermint/config/config.toml`) to update the `proxy_app` and P2P `laddr` as follows.
```toml
proxy_app = "tcp://127.0.0.1:26358"
# ...
[p2p]
laddr = "tcp://0.0.0.0:26356"
```

### Step 3: Module specific setup
See the module documentation for more details -
* [Bank module](docs/modules/bank.md)
* [Ibc module](docs/modules/ibc.md)

### Step 4: Run the basecoin app and Tendermint
```shell
# See all supported CLI options
$ cargo run -- --help
tendermint-basecoin 0.1.0

USAGE:
    tendermint-basecoin [FLAGS] [OPTIONS]

FLAGS:
        --help       Prints help information
    -q, --quiet      Suppress all output logging (overrides --verbose)
    -V, --version    Prints version information
    -v, --verbose    Increase output logging verbosity to DEBUG level

OPTIONS:
    -g, --grpc-port <grpc-port>            Bind the gRPC server to this port [default: 9093]
    -h, --host <host>                      Bind the TCP server to this host [default: 127.0.0.1]
    -p, --port <port>                      Bind the TCP server to this port [default: 26658]
    -r, --read-buf-size <read-buf-size>    The default server read buffer size, in bytes, for each incoming client
                                           connection [default: 1048576]

# Run the ABCI application (from this repo)
# The -v is to enable trace-level logging
$ cargo run -- -v

# In another terminal
$ tendermint node
```

## UML diagrams
![system diagram](docs/images/system-diagram.png)
---
![class diagram](docs/images/class-diagram.png)
---
![activity diagram - DeliverTx](docs/images/activity-diagram-deliverTx.png)
