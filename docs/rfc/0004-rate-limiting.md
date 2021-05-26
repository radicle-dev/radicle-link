# RFC: Connection Sentinels

* Author: @alexjg
* Date: 2021-05-25
* Status: draft

## Motivation

We have observed problems when running the seed wherein some misbehaving peers
create many incoming connections to the seed node consuming all the CPU on the
node and causing it to become unresponsive. There are multiple solutions to
this - rate limiting, allow/deny lists, more complex reputation systems etc.
To support these solutions `librad` must expose a mechanism for determining 
whether to accept a connection.

## What to filter

The librad network stack runs several different services, each of these has
different performance characteristics and is likely to warrant different
filtering policies. On that basis we expose the following interface for
applications to decide whether a connection or a stream should be accepted

We introduce the following types


```rust
enum StreamType {
    Git,
    Membership,
    Gossip,
    Interogation,
}

enum Decision {
    Allow,
    Deny{reason: String},
}

trait ConnectionSentinel<P: RemoteAddr + RemotePeer> {
    fn connection_established(&mut self, peer: &P)
    fn stream_opened(&mut self, peer: P, stream_type: StreamType)
    fn allow_connection(&mut self, peer: P) -> Decision
    fn allow_stream(&mut self, peer: P, stream_type: StreamType) -> Decision
}
```

An implementation of `ConnectionSentinel` can be passed via 
`librad::net::peer::Config` to `librad::net::peer::Peer`. This 
`ConnectionSentinel` is notified when a connection is established and when 
new streams are opened. `allow_connection` is called whenever a new connection
might be established and if it returns `Decision::Deny` then the connection is
rejected. `allow_stream` is called whenever a new stream might be opened (
sepcifically in `librad::net::protocol::io::streams::incoming`), in the case of
a `Deny` decision then `incoming` returns an error, terminating the entire 
connection.

### Terminating Connections

Terminating a connection when `Deny` is returned from `allow_stream` will mean
a bunch of spurious error messages from other streams on the same connection as
they will be abruptly closed. An alternative approach might be just to deny
opening this stream and leave the rest of the connection intact. This would
potentially allow the peer to open a stream later after some rate limiting
window has expired. The current peer implementation doesn't behave like this
though, it just closes the entire connection if the open stream fails. This 
seems like simpler behaviour which will be easier to debug.


