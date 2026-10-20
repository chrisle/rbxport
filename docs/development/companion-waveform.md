# Companion waveform overview

The `read waveform overview` automation command is read-only. Its JSON input
contains `library_id` (from `my tag catalog`) and `track_id`, both strings. The
catalog advertises `waveform.overview.read` so companions can detect older builds.

The reply contains `schema_version: 1`, matching IDs, `kind: "bands"`, and a
`data` byte array: low/mid/high triples from the existing PWV6 three-band overview.
At most 1,200 columns / 3,600 bytes are returned. An empty array means that no
three-band overview was found; no analysis is started. A mismatched library ID
is rejected. This does not expose arbitrary filesystem reads or database writes.

A companion adapter should validate the version, identities, kind and bounded bytes. It can keep a small in-memory cache and use the audio element's duration for seeking.
An ordinary seek slider should remain available for keyboard access or missing data.
