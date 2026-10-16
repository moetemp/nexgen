The Python HTTP caller takes `payload_codec=`. It encodes each input as a Temporal payload and decodes an `application/x-temporal-payload` answer before it parses, so a caller reads a result that the handler's codec encoded.
The Python HTTP caller takes `ssl_context=`, which sets the TLS for an `https` endpoint, such as a private certificate authority or a client certificate for mTLS.
The Python HTTP caller's `HTTPStatusError` has `failure_type` and `failure_message`, read from the application failure in a Nexus failure body, so a caller can raise the error class the handler raised.
