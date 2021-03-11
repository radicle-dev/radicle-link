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

This RFC is built on top of the [identities spec][identities].
It introduces support for Ethereum address claims on Link
and a smart contract on Ethereum to make Link identity claims on Ethereum.

## Link identity JSON extension

The identity doc `payload` structure supports a new key: `https://radicle.xyz/ethereum/claim/v1`.
It can be used only in person identities.
Under this key there is stored an Ethereum address claim in this format:

- `address` - the claimed Ethereum address, encoded according to [EIP-55][eip-55],
e.g. using [ethers.js][ethers-addr]
- `expiration` - the claim expiration timestamp, encoded as a [JavaScript Date][date]

Example:
```json
{
    "payload": {
        "https://radicle.xyz/ethereum/claim/v1": {
            "address": "0xAb5801a7D398351b8bE11C439e05C5B3259aeC9B",
            "expiration": "2021-03-19T23:15:30.000Z"
        }
        ...
    }
    ...
}
```

## Ethereum smart contract

A new Ethereum smart contract is deployed to the network,
which lets users claim their Link identities:

```solidity
contract Claims {
    event Claimed(address indexed addr);
    function claim(uint8 version, uint256 root) public;
}
```

To claim an identity, call `claim` using your Ethereum account and pass your Link identity root.
It will emit an event `Claimed`, which later can be queried to discover your attestation.
The claims have no expiration date and don't need to be renewed.

Every new claim invalidates previous ones made with the same account.
To only revoke a claim without creating a new one, use root `0`,
which is guaranteed to not match any existing identity.

Currently supported `version` values:
- `1` - `root` is a SHA-1 root hash. The excess high bytes are zeros, e.g. for hash
`fb3102b74d7254eed7f18a31a3ba1ea946bb1a99` the passed `root` is
`000000000000000000000000fb3102b74d7254eed7f18a31a3ba1ea946bb1a99`
- `2` - `root` is a SHA-256 root hash

We need to deploy an official instance of the `Claims` smart contract and
it must be used by all the users.
If anybody makes a claim using a different instance, it won't be recognized by others.

## Creation of an attestation

You need to perform 2 actions in any order:
- Add or update an `https://radicle.xyz/ethereum/claim/v1` entry in your identity doc.
The entry's `address` must be your Ethereum address.
It's highly recommended to set a short expiration date as Ethereum claims don't expire.
- Call `claim` in the `Claims` smart contract. The `root` must point to your link identity.

## Discovery from an Ethereum address

When you know an Ethereum address, you can find the claimed link identity using an Ethereum client.
The example calls are based on the standard [client JSON RPC API][rpc] and should be exposed
by your favourite Ethereum client library.
It's important that the client must be trusted not to hide the events.

- Use [getLogs][rpc-logs] to get the newest `Claimed` event filtered for the given ethereum address
- Get the event's `transactionHash` field and use it to fetch the transaction which emitted it with
[getTransactionByHash][rpc-tx]
- Validate that the transaction signature matches its data and the ethereum address.
For reference the signature payload content is listed [here][rpc-sign].
- Read the Link identity root from the transaction data
- Verify that the Link identity doc claims back the Ethereum address,
see [Discovery from a Link identity](#discovery-from-a-link-identity)

## Discovery from a Link identity

When you know a Link identity, you can find the claimed Ethereum address.
Obtain the tip of its identity chain and read the Ethereum address from the identity doc
`address` field in section `https://radicle.xyz/ethereum/claim/v1`, unless it's expired.
You need to verify that the given Ethereum address claims back the Link identity root,
see [Discovery from an Ethereum address](#discovery-from-an-ethereum-address).

## Revocation of an attestation

When your attestation for whatever reason is no longer valid,
you should revoke it as soon as possible.
Only one claim needs to be revoked to break the attestation,
but to improve security you should revoke both sides if you can.

To revoke a claim on Link, update the identity doc and publish it in a new revision.
You can change the claimed Ethereum address or remove
the `https://radicle.xyz/ethereum/claim/v1` section altogether.
The other Link nodes will not notice this update until they fetch the new revision.

To revoke a claim on Ethereum, call the `claim` function in `Claims` contract.
You can claim a different Link identity root or the `0` root to revoke any claim you may have.
The other users will notice this update almost immediately if they
are subscribed to the `Claimed` events for your Ethereum address in their Ethereum client.

---

[identities]: ../spec/sections/002-identities/index.md
[eip-55]: https://eips.ethereum.org/EIPS/eip-55
[date]: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date/toJSON
[ethers-addr]: https://docs.ethers.io/v5/api/utils/address/
[rpc]: https://eth.wiki/json-rpc/API
[rpc-logs]: https://eth.wiki/json-rpc/API#eth_getlogs
[rpc-tx]: https://eth.wiki/json-rpc/API#eth_gettransactionbyhash
[rpc-sign]: https://eth.wiki/json-rpc/API#eth_signtransaction
