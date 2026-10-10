Python proto converters no longer call `HasField` on repeated or map fields, which raised `ValueError` for repeated messages and maps.
