use std::net::SocketAddr;

use librad::{meta::project::ProjectInfo, net::peer::PeerApi, peer::PeerId, uri::RadUrn};

use crate::{guess_user, Error, Project, Signer};

/// An event generated by the seed node.
#[derive(Debug, Clone)]
pub enum Event {
    /// The seed node is listening for peer connections.
    Listening(SocketAddr),
    /// A peer has connected.
    PeerConnected {
        peer_id: PeerId,
        urn: Option<RadUrn>,
        name: Option<String>,
    },
    /// A peer has disconnected.
    PeerDisconnected(PeerId),
    /// A project has been tracked from a peer.
    ProjectTracked(Project, PeerId),
}

impl Event {
    pub(crate) async fn peer_connected(
        peer_id: PeerId,
        api: &PeerApi<Signer>,
    ) -> Result<Self, Error> {
        let user = self::guess_user(peer_id, api).await?;
        let user = user.as_ref();

        Ok(Self::PeerConnected {
            peer_id,
            urn: user.map(|u| u.urn()),
            name: user.map(|u| u.name().to_owned()),
        })
    }

    pub(crate) async fn project_tracked(
        urn: RadUrn,
        provider: PeerId,
        api: &PeerApi<Signer>,
    ) -> Result<Self, Error> {
        let proj = api
            .with_storage({
                let urn = urn.clone();
                move |s| s.metadata_of::<ProjectInfo, _>(&urn, provider)
            })
            .await??;

        Ok(Event::ProjectTracked(
            Project {
                urn: urn.clone(),
                maintainers: proj.maintainers().clone(),
                name: proj.name().to_owned(),
                description: proj.description().to_owned(),
            },
            provider,
        ))
    }
}
