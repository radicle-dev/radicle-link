# CLIB

When a CLI needs a LIB, one gets a CLIB.

This package provides the common data types and functions that can be
used across the different set of CLI packages.

* `clib::keys` - common functions for setting up of and retrival from the secret key
  storage.
* `clib::ser` - serialization formats required for CLI output.
* `clib::storage` - common functions for setting up read-only and
  read-write storage.
