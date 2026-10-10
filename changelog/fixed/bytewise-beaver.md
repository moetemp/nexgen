WIT `list<u8>` is read as a byte string, so it matches a proto `bytes` field and generates `bytes`, `Uint8Array` or `[]byte` instead of a list of integers.
