# RFC: Ethereum attestation

* Author: @CodeSandwich
* Date: 2021-03-05
* Status: draft
* Community discussion: n/a

## Motivation

The attestation between Link and Ethereum is a valuable building block for a user identity.
It brings the Link reputation coming from projects and contributions to the
Ethereum world of DAOs and donations, where it's important to know who's behind an address.
On the other hand, it lends Ethereum account reputation with its assets and undeniable history
to Link to build user's trust in the identity.

## Overview

This RFC is built on top of [Identity Proofs RFC][rfc].
It introduces support for Ethereum address claims on Link
and a smart contract on Ethereum to make Link identity claims on Ethereum.

## Link identity JSON extension

The identity JSON supports a new key: `ethereum`.
Under this key there is stored an ethereum address claim following this convention:

- `account` - the claimed ethereum address, encoded according to [EIP-55][eip-55],
e.g. using [ethers.js][ethers-addr]
- `proof` - not set (according to [Identity Proofs RFC][rfc] it's an optional field)

Example:
```json
{
    "ethereum": {
        "account": "0xAb5801a7D398351b8bE11C439e05C5B3259aeC9B",
        "expiration": {
            "created": 1614768373,
            "expires": 31536000
        }
    }
}
```

## Ethereum smart contract

A new Ethereum smart contract is deployed to the network, which lets users claim their Radicle IDs:

```solidity
contract Claims {
    event Claimed(address indexed addr);
    function claim(uint8 version, uint256 id) public;
}
```

To claim an ID, call `claim` using your Ethereum account.
It will emit an event `Claimed`, which later can be queried to discover your attestation.
The claims have no expiration date and don't need to be renewed.

Every new claim invalidates previous ones made with the same account.
To only revoke a claim without creating a new one, use ID `0`,
which is guaranteed to not match any existing identity.

Currently supported `version` values:
- `1` - an `id` is a SHA-1 root hash. The excess high bytes are zeros, e.g. for hash
`fb3102b74d7254eed7f18a31a3ba1ea946bb1a99` the passed `id` is
`000000000000000000000000fb3102b74d7254eed7f18a31a3ba1ea946bb1a99`
- `2` - an `id` is a SHA-256 root hash

We need to deploy an official instance of the `Claims` smart contract and
it must be used by all the users.
If anybody makes a claim using a different instance, it won't be recognized by others.

## Creation of an attestation

You need to perform 2 actions in any order:
- Add or update an `ethereum` entry in your identity JSON.
The entry's `account` must be your Ethereum address.
It's highly recommended to set a reasonable expiration date as Ethereum claims don't expire.
- Call `claim` in the `Claims` smart contract. The `id` must point to your link identity.

## Discovery from an Ethereum address

When you have an Ethereum address, you can find the claimed link ID using an Ethereum client.
The example calls are based on the standard [client JSON RPC API][rpc] and should be exposed
by your favourite Ethereum client library.
It's important that the client must be trusted not to hide the events.

- Use [getLogs][rpc-logs] to get the newest `Claimed` event filtered for the given ethereum address
- Get the event's `transactionHash` field and use it to fetch the transaction which emitted it with
[getTransactionByHash][rpc-tx]
- Validate that the transaction signature matches its data and the ethereum address.
For reference the signature payload content is listed [here][rpc-sign].
- Read the link ID from the transaction data
- Verify that the link ID claims back the Ethereum address,
see [Discovery from a Link ID](#discovery-from-a-link-id)

## Discovery from a Link ID

When you have a link ID, you can find the claimed Ethereum address.
Obtain the tip of its identity chain and read the ethereum address from the identity JSON
`account` field in section `ethereum`, unless it's expired.
You need to verify that the given Ethereum address claims back the link ID,
see [Discovery from an Ethereum address](#discovery-from-an-ethereum-address).

## Revocation of an attestation

When your attestation for whatever reason is no longer valid,
you should revoke it as soon as possible.
Only one claim needs to be revoked to break the attestation,
but to improve security you should revoke both sides if you can.

To revoke a claim on Link, update and publish the identity JSON.
You can change the claimed Ethereum address or remove the `ethereum` section altogether.

To revoke a claim on Ethereum, call the `claim` function in `Claims` contract.
You can claim a different link ID or an ID `0` to revoke any claim you may have.

---

[rfc]: ./identity_proofs.md
[eip-55]: https://eips.ethereum.org/EIPS/eip-55
[ethers-addr]: https://docs.ethers.io/v5/api/utils/address/
[rpc]: https://eth.wiki/json-rpc/API
[rpc-logs]: https://eth.wiki/json-rpc/API#eth_getlogs
[rpc-tx]: https://eth.wiki/json-rpc/API#eth_gettransactionbyhash
[rpc-sign]: https://eth.wiki/json-rpc/API#eth_signtransaction
