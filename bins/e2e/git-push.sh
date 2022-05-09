#!/usr/bin/env bash
set -eou pipefail

# Ignore adding localhost to ~/.ssh/known_hosts
export GIT_SSH_COMMAND="ssh -o UserKnownHostsFile=/dev/null -o StrictHostKeyChecking=no"

# Set up home directories in /tmp
mkdir -p /tmp/lnk-test-peer
mkdir -p /tmp/lnk-test-seed
mkdir -p /tmp/lnk-bins
PEER_HOME=/tmp/lnk-test-peer
SEED_HOME=/tmp/lnk-test-seed
BINS=/tmp/lnk-bins

echo "ensuring ${PEER_HOME} is created"
echo "ensuring ${SEED_HOME} is created"
echo "ensuring ${BINS} is created"

cwd="$(dirname "$(readlink -nf "${BASH_SOURCE[0]}")")/.."
cd $cwd

# Build binaries
echo "building lnk"
cargo install --path="$cwd/lnk" --debug --root=$BINS --offline

echo "building lnkd"
cargo install --path="$cwd/linkd" --debug --root=$BINS --offline

echo "building gitd"
cargo install --path="$cwd/lnk-gitd" --debug --root=$BINS --offline

lnk=$BINS/bin/lnk
linkd=$BINS/bin/linkd
gitd=$BINS/bin/lnk-gitd

# Check if profiles for the peer and seed were created, and if not
# create them. Note that this prompts for passwords to be input.
if [[ -f $PEER_HOME/active_profile ]]
then
   echo "peer profile already exists"
else
    echo "creating peer profile"
    LNK_HOME=$PEER_HOME $lnk profile create
fi

if [[ -f $SEED_HOME/active_profile ]]
then
   echo "seed profile already exists"
else
    echo "creating seed profile"
    LNK_HOME=$SEED_HOME $lnk profile create
fi

# Check if keys for the peer and seed were added to the ssh agent, and
# if not add them. Note that this prompts for the password that was
# set during profile creation.
echo "checking if peer key is ready"
EXIT_CODE=0
LNK_HOME=$PEER_HOME $lnk profile ssh ready || EXIT_CODE=$?
if [[ $EXIT_CODE -ne 0 ]]
then
    echo "please add peer key"
    LNK_HOME=$PEER_HOME $lnk profile ssh add
fi

echo "checking if seed key is ready"
EXIT_CODE=0
LNK_HOME=$SEED_HOME $lnk profile ssh ready || EXIT_CODE=$?
if [[ $EXIT_CODE -ne 0 ]]
then
    echo "please add seed key"
    LNK_HOME=$SEED_HOME $lnk profile ssh add
fi

echo "retrieving peer ids"
SEED_ID=$(LNK_HOME=$SEED_HOME $lnk profile peer)
PEER_ID=$(LNK_HOME=$PEER_HOME $lnk profile peer)

# Set up the local identity for the peer
EXIT_CODE=0
LNK_HOME=$PEER_HOME $lnk identities local default || EXIT_CODE=$?
if [[ $EXIT_CODE -ne 0 ]]
then
    echo "setting peer identity"
    urn=$(LNK_HOME=$PEER_HOME $lnk identities person create new \
                  --payload '{"name": "sockpuppet"}' | jq -r '.urn')
    LNK_HOME=$PEER_HOME $lnk identities local set --urn $urn
fi

# Set up the project for the peer
if [ ! -d $PEER_HOME/jnk ]
then
    echo "creating project"
    project=$(LNK_HOME=$PEER_HOME $lnk identities project create new \
                      --path $PEER_HOME \
                      --payload '{"name": "jnk"}' | jq -r '.urn')
fi

# Set up the seed entry for the peer
PEER_PROFILE=$(LNK_HOME=$PEER_HOME $lnk profile get)
if [ ! -s $PEER_HOME/$PEER_PROFILE/seeds ]
then
    echo "adding seed to seeds config"
    echo "${SEED_ID}@127.0.0.1:8899" > $PEER_HOME/$PEER_PROFILE/seeds
fi

# Start up daemon servers in a forked process
$linkd \
    --lnk-home $SEED_HOME \
    --track everything \
    --protocol-listen 127.0.0.1:8899 \
    > $SEED_HOME/seed.logs &
LINKD_PID=$!

systemd-socket-activate \
    -l 9987 \
    --fdname=ssh \
    -E SSH_AUTH_SOCK \
    -E RUST_BACKTRACE \
    -E RUST_LOG \
    $lnk-gitd \
    $PEER_HOME \
    --linkd-rpc-socket $XDG_RUNTIME_DIR/link-peer-$PEER_ID-rpc.socket \
    --linger-timeout 10000 \
    --push-seeds \
    > $PEER_HOME/gitd.logs &
GITD_PID=$!

cleanup() {
    echo "cleaning up servers"
    kill $GITD_PID
    kill $LINKD_PID
}
trap 'cleanup' EXIT

echo "waiting for servers to initialise"
sleep 2

# Add the remote if needed, add data to a README file and push the
# changes
cd $PEER_HOME/jnk

echo "checking if linkd remote exists"
EXIT_CODE=0
git config --get remote.linkd.url || EXIT_CODE=$?
if [[ $EXIT_CODE -ne 0 ]]
then
    echo "adding linkd remote"
    git remote add linkd ssh://rad@127.0.0.1:9987/$project
fi

echo "committing data and pushing"
echo $RANDOM | md5sum > README.md
git add README.md
git commit -m "Updating README"
git push linkd main
