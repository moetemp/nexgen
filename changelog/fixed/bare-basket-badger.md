Proto-backed WIT records accept a bare `list<T>` or `map<K, V>` for a repeated or map field. `add-rpc` writes this form, and validation rejected it as a required field.
