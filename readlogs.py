"""
This is a script for making the logs from the rere_tracked scenario test a
bit more readable. It replaces all mentions of the peer IDs in the test with
<Peer 1> or <Peer 2> and formats fetchspecs from fetcher logs nicely. To use it
first invoke the test in the following way:

>>> RUST_LOG=radicle_link_test::test::integration::librad::scenario::rere_tracked=trace,librad::git::replication=trace,librad::net::protocol::io::recv::git=info,librad::git::storage::fetcher=trace,librad::git::refs=debug,librad::net::protocol::io::graft=trace cargo test rere_tracked | tee logs

Then run the script:

>>> python readlogs.py logs
"""
import re
from typing import Dict, List, Tuple
import sys

class RefTable:
    prefix: str
    entries: List[Tuple[str, str]]

    def __init__(self, prefix: str):
        self.prefix = prefix
        self.entries = []

    def add_entry(self, fromref: str, toref: str):
        self.entries.append((fromref.strip(), toref.strip()))

    def render(self, indent=8) -> List[str]:
        max_fromlen = max([len(f) for (f, _) in self.entries])
        result = [self.prefix]
        for (fromref, toref) in self.entries:
            padding_len = max_fromlen - len(fromref)
            padding = " " * padding_len
            indent_padding = " " * indent
            result.append(f"{indent_padding}{fromref}{padding} --> {toref}")
        return result


def extract_peers(logs) -> Dict[str, str]:
    peer_re = re.compile(
        r"radicle_link_test::test::integration::librad::scenario::rere_tracked:created peers peer1=(\S+) peer2=(\S+)"
    )
    m = peer_re.search(logs)
    if m is not None:
        peer_1 = m.groups()[0]
        peer_2 = m.groups()[1]
        return {
            peer_1: "Peer 1",
            peer_2: "Peer 2",
        }
    else:
        raise Exception("No match for peers re")


project_id_re = re.compile(r"project_id=\"([^\"]+)\"")
def extract_project(logs) -> str:
    if (m := project_id_re.search(logs)) is not None:
        return m.groups()[0]
    else:
        raise Exception("No match for project ID regex")


ref_re = re.compile(r"imp:\[([^\]]+)\]")
def expand_fetchspec_lines(log_line: str) -> List[str]:
    if "librad::git::storage::fetcher::imp:" in log_line \
            and "{fetchspecs}" in log_line:
        if (m := ref_re.search(log_line)) is not None:
            prefix = log_line[:m.start()] + "imp:"
            reftable = RefTable(prefix)
            refs_raw = m.groups()[0]
            refs_raw = refs_raw.replace("\"", "")
            reflines = refs_raw.split(", ")
            for line in reflines:
                (fromref, toref) = line.split(":")
                reftable.add_entry(fromref, toref)
            return reftable.render()
    return [log_line]


if __name__ == "__main__":
    with open(sys.argv[1]) as infile:
        logs = infile.read()
    peers = extract_peers(logs)
    project_id = extract_project(logs)
    for (peer_id, peer) in peers.items():
        logs = logs.replace(peer_id.strip(), f"<{peer}>")
    logs = logs.replace(project_id, "<project>")
    log_lines = logs.split("\n")
    log_lines = [l for line in log_lines for l in expand_fetchspec_lines(line)]
    for line in log_lines:
        if line.endswith("pulling"):
            print("---------------------------" * 2)
            print("\n\n\n\n")
        print(line)
    for (peer_id, peer) in peers.items():
        print(f"{peer}: {peer_id}")
    print(f"Project: {project_id}")

