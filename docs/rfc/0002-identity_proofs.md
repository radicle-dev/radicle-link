# RFC: Identity Proofs

* Author: @kim
* Date: 2021-02-23
* Status: draft
* Community discussion: n/a

## Motivation

A `radicle-link` [identity][ids] is a hash-linked sequence of signed statements
about public-key delegations: if an entry in this sequence is found to conform
to a quorum rule of cryptographic signatures, the set of keys it delegates to
can be considered trustworthy _iff_ the previous set was. Yet, how can we trust
the initial set of keys in this chain?

## Overview

We consider it impractical for most participants in the `radicle-link` network
to exchange public keys out-of-band, given a pattern of casual interaction with
others. While the protocol mandates connections between participants, similar to
"following" relationships found in social media, we thus consider it
insufficient to infer a [web of trust][wot] from those relationships.

To lift the requirement for physical authenticity checks, but still increase
confidence of a given public key being associated with a particular person,
[keybase] have popularised a scheme dubbed "Social Proofs": a statement claiming
ownership of a particular account on a social media site is written to the
keybase "sigchain" (which has properties similar to a `radicle-link` identity).
This claim (implying also the history of the sigchain) is signed using a key
currently valid according to the chain, and the signature (along with the public
key) is stored at the social media site. To verify the claim, the signature is
retrieved from the social media site in such a way that it could _plausibly_
only be created by the owner of the claimed account. If both the sigchain
integrity and the claim signature can be verified, the association is proven.

This mechanism can be considered a practical application of the [Turing
test][tt]: even though it can not be proven beyond doubt that the account is
indeed associated with a real person, the evidence of others accepting it as
such, as well as conversational behaviour, can increase the confidence in the
authenticity of the online persona. Because key and account ownership at a given
point in time can be cryptographically verified, this confidence can be extended
to the proving side.

We conclude that this mechanism would be a good fit for `radicle-link` (due to
the similarities), and a desirable feature of applications built on top of it.

The following sections describe how such claims shall be stored in the identity
payload of a `radicle-link` identity, how to obtain a publishable proof, and how
to verify it.

## Claims

Claims can only be made by identities of kind `Person`, and claim a single
external account identifier. They are introduced by defining a new payload type,
identified by the URL:

    https://radicle.xyz/link/claim/v1

The shape of the JSON structure is:

```json
{
    "SERVICE": {
        "account": "STRING"
        "expiration": {
            "created": INTEGER,
            "expires": INTEGER
        },
        "proof": "URL"
    }
}
```

Where the fields denote:

* `SERVICE`

  A conventional identifier of the external service, e.g. "github", "twitter",
  "radicle-ethereum"

* `account`

  The unique account identifier within the service, using the service-specific
  canonical string representation e.g. "kim", "0x32be343b94f860124dc4fee278fdcbd38c102d88".

* `expiration` (optional)
    * `created`

      Creation timestamp of the claim, in seconds since 1970-01-01T00:00:00Z.

    * `expires`

      Seconds relative to `created`, after which the claim should no longer be
      considered.

* `proof` (optional)

  A URL to assist verification tooling in retrieving the proof from the external
  system. This is mainly a convenience, and obviously requires creation of a new
  revision after the fact.

## Proof Generation

The above claim payload is committed to the identity history as a new revision.
Technically, this revision needs to be approved by all key delegations for
verification to pass later on, but since we assume that eligible keys are held
by the same person, it may be acceptable to publish the proof right away for
user experience reasons.

The actual proof consists of the following tuple:

    (root, revision, public-key, signature)

Note that the "git" protocol specifier of `radicle-link` URNs is implied, that
is, future version MUST treat the absence of a disambiguating value as denoting
"git".

The values `root`, `revision`, and `public-key` are specified in
[identities][ids], and it is RECOMMENDED to follow the serialisation formats
devised there. `signature` is the Ed25519 signature over `revision`, in much the
same way as the actual revision is signed. All values can thus be obtained by
inspecting the identity storage.

It is beyond the scope of this document to devise the exact external format to
serialise the tuple into, as this is expected to vary from service to service.

## Revocation

A claim can be revoked by creating a new identity revision which simply does not
contain the claim payload. Likewise, a later claim describing the same `SERVICE`
invalidates an earlier one.

## Verification

Inputs: the 4-tuple as specified [above](#proof-generation), and `(SERVICE,
account)` as inferred from the source it was retrieved from.

1. Given the 4-tuple specified, it is first verified that the signature is valid
   for the given `revision` and `public-key`.

2. If it is, the identity history needs to be resolved from local storage, or
   the network.

   Using `git` storage, the history tip should be located at

        refs/namespaces/<root>/refs/remotes/<public-key>/rad/id

   substituting `<root>` and `<public-key>` with their respective encodings as
   defined in [`Identities`][ids].

3. If the history tip could be resolved, the identity MUST be verified as per
   [`Identities`][ids]. If this fails, the proof is rejected.

4. If the identity could be verified, the identity document is read from its
   latest valid tip (recall that this is not necessarily the same as what the
   ref points to). The proof is rejected if one of the following is true:

   4.1 `root` does not match

   4.2 the document does not contain a claim for `(SERVICE, account)`

   4.3 the document contains a claim for `(SERVICE, account)`, has an
       `expiration`, and `expiration.expires` is smaller than `time() -
       expiration.created`

5. Lastly, the identity history is walked backwards until `revision` is found
   (or else, the proof is rejected). The proof is accepted _iff_ all of the
   following are true:

   5.1 the document's `delegations` at `revision` contain `public-key`

   5.2 the document at `revision` contains a claim for `(SERVICE, account)`, and
       the claim is not expired as described in 4.3

Note that steps 3.-5. can be optimised by persisting verification results, or by
adding an additional accumulator to the verification fold which yields the
targeted `revision`.

## Discussion

The inclusion of the `revision` in the proof allows to assert that `root` is
indeed an ancestor, which opens up another way to detect "forks" of the identity
history: due to the peer-to-peer nature of the `radicle-link` network, it is
vulnerable to attacks which involve withholding data from other participants, in
which case a fork may go unnoticed.

It should be noted, however, that refreshing the proof from time to time in
order to ensure freshness of the data retrieved through `radicle-link` is not
always practicable.

In order to prove that the(ir own) server is not lying by omission, Keybase
[anchors a merkle root][keybase-stellar] on a blockchain, which includes all
sigchains registered in the Keybase directory. Because `radicle-link` does not
have such a central directory, this approach could only be applied to a partial
view of the network.

While conceivable that, given the right incentives, such a directory service
could be operated independently (similar to what the [ceramic] network devises),
it is unclear what value blockchain anchors of individual identities have, given
that transaction costs discourage frequent updates.

We thus RECOMMEND to explore Layer 2 solutions for blockchain anchoring.

---

[ids]: ../spec/sections/002-identities/index.md
[wot]: https://en.wikipedia.org/wiki/Web_of_trust
[keybase]: https://book.keybase.io/account#proofs
[tt]: https://en.wikipedia.org/wiki/Turing_test
[keybase-stellar]: https://book.keybase.io/docs/server/stellar
[ceramic]: https://github.com/ceramicnetwork/ceramic/blob/master/SPECIFICATION.md#blockchain-anchoring
[radicle-contracts]: https://github.com/radicle-dev/radicle-contracts
