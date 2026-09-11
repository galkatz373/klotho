# KAI v1 benchmark governance

One immutable `OutcomeId` represents one request across every operation, retry,
file, and commit. Timeouts, refusals, invalid output, human takeover, and
exhausted repair remain failed trials.

The public corpus is 70% of 100 mechanic, 200 content, and 50 asset outcomes.
Gal Katz is the benchmark owner and keeps the 30% cleartext prompts in the
offline access-controlled project vault identified by the receipt in
`held-out.hashes`; Git contains only SHA-256 prompt commitments. The benchmark
owner may not implement the evaluated agent policy.

Trials run in OutcomeId order: five per public outcome and three per held-out
outcome. The monotonic wall clock starts at request submission and ends only
when an evidence-complete candidate enters review. Corpus exclusions, promotion,
or difficulty changes require owner approval and a new major version. Promotion
verifies prompt commitments, publishes the old/new migration note, and never
mutates v1 results. Reports without every environment hash are telemetry, not
evidence.

KAI-00 locks a deterministic P0 protocol fixture lane because KAI-07 has not
landed. Its `not_run` metrics cannot green AI quality or latency claims. A real
provider creates a new lock and benchmark lane; it never overwrites this one.
