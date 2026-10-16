Python proto converters no longer rebind `value` while writing a repeated or map message field, which broke every field written after it.
