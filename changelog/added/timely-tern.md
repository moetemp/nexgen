Each Python client call takes `rpc_metadata` and `rpc_timeout`, and passes them to `_create_nexus_client`. The methods of a resource take them too, and its Workflow operations raise `ValueError` when either is set.
An operation that WIT declares `async func` also gets the Python client method `start_<name>`, which returns the handle of the operation.
