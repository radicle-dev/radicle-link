use radicle_seed::{Node, NodeConfig};

use futures::executor;

fn main() {
    let config = NodeConfig::default();
    let node = Node::new(config).unwrap();

    executor::block_on(node.run()).unwrap();
}
