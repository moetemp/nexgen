A Python model of a WIT-only record that holds a `duration` encodes it as a proto3 JSON duration, such as `"1.5s"`, since the SDK JSON converter can't encode a `timedelta`.
