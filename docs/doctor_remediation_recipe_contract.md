# doctor Remediation Recipe DSL Contract

## Scope

`asupersync doctor remediation-contract` emits the machine-readable remediation DSL contract for `doctor_asupersync` Track 4 workflows.

This contract defines:

- deterministic recipe schema for fix intents, preconditions, rollback plans, and confidence inputs
- confidence scoring model (weighted inputs in basis points)
- risk band policy for `apply` vs. `review` decisioning
- compatibility/versioning guidance for future DSL evolution
- deterministic fixture bundle for parser/validator/scorer regression testing

## Command

```bash
asupersync doctor remediation-contract
```

## Contract Version

- `contract_version`: `doctor-remediation-recipe-v1`
- Depends on logging contract: `doctor-logging-v1`
- Backward-compatible additive fields are allowed within `v1`
- Any semantic changes to required fields, scoring math, or risk-band semantics require a version bump

## Output Schema

```json
{
  "contract": {
    "contract_version": "doctor-remediation-recipe-v1",
    "logging_contract_version": "doctor-logging-v1",
    "required_recipe_fields": [
      "confidence_inputs",
      "finding_id",
      "fix_intent",
      "preconditions",
      "recipe_id",
      "rollback"
    ],
    "required_precondition_fields": [
      "evidence_ref",
      "expected_value",
      "key",
      "predicate",
      "required"
    ],
    "required_rollback_fields": [
      "rollback_command",
      "strategy",
      "timeout_secs",
      "verify_command"
    ],
    "required_confidence_input_fields": [
      "evidence_ref",
      "key",
      "rationale",
      "score"
    ],
    "allowed_fix_intents": ["..."],
    "allowed_precondition_predicates": ["contains", "eq", "exists", "gte", "lte"],
    "allowed_rollback_strategies": ["..."],
    "confidence_weights": [
      {"key": "analyzer_confidence", "weight_bps": 3200, "rationale": "..."}
    ],
    "risk_bands": [
      {
        "band_id": "critical_risk",
        "min_score_inclusive": 0,
        "max_score_inclusive": 39,
        "requires_human_approval": true,
        "allow_auto_apply": false
      }
    ],
    "compatibility": {
      "minimum_reader_version": "doctor-remediation-recipe-v1",
      "supported_reader_versions": ["doctor-remediation-recipe-v1"],
      "migration_guidance": [{"from_version": "doctor-remediation-recipe-v0", "to_version": "doctor-remediation-recipe-v1", "breaking": false, "required_actions": ["..."]}]
    }
  },
  "fixtures": [
    {
      "fixture_id": "fixture-guarded-auto-apply",
      "description": "...",
      "recipe": {"recipe_id": "recipe-*", "finding_id": "...", "fix_intent": "...", "preconditions": ["..."], "rollback": {"...": "..."}, "confidence_inputs": ["..."]},
      "expected_confidence_score": 80,
      "expected_risk_band": "guarded_auto_apply",
      "expected_decision": "apply"
    }
  ]
}
```

## Determinism and Validation Rules

`validate_remediation_recipe_contract` enforces:

1. lexical ordering + uniqueness of deterministic string arrays
2. required recipe fields are present
3. confidence weights are non-zero and sum to exactly `10_000` bps
4. risk bands are contiguous and gap-free over `0..=100`
5. compatibility metadata is complete and migration actions are deterministic

`validate_remediation_recipe` enforces:

1. `recipe_id` must be a `recipe-*` slug
2. `fix_intent`, predicates, and rollback strategy must be in contract allowlists
3. preconditions and confidence inputs must be lexically ordered and unique by key
4. rollback commands must be single-line command strings with non-zero timeout
5. confidence inputs must provide required evidence references and per-input rationale

`parse_remediation_recipe` fails closed on invalid JSON or schema violations.

## Confidence Scoring Model

`compute_remediation_confidence_score` computes:

```text
score = floor(sum(input_score * weight_bps) / 10_000)
```

Where:

- each `input_score` is in `0..=100`
- `weight_bps` values come from the contract
- contributions are emitted as deterministic trace strings

Risk band selection is policy-driven by score interval. Output includes:

- `confidence_score`
- `risk_band`
- `requires_human_approval`
- `allow_auto_apply`
- `weighted_contributions`

## Structured Logging Expectations

`run_remediation_recipe_smoke` emits deterministic remediation-flow events via `doctor-logging-v1`:

- `remediation_apply`
- `remediation_verify`
- `verification_summary`

Events include rule-evaluation context, confidence contributions, and rejection or override rationale fields when applicable, with stable `run_id`/`scenario_id`/`trace_id` correlation.

## Safe Extension Strategy

1. Additive only within `doctor-remediation-recipe-v1`:
   - new optional recipe metadata fields
   - new fixture entries
   - additional fix intents/predicates/rollback strategies (must stay lexical and validated)
2. Version bump required for:
   - required field changes
   - confidence weight semantics or score formula changes
   - risk-band decision policy changes
3. Consumers should:
   - fail closed on unknown contract versions
   - validate contract + recipe payloads before execution
   - persist emitted confidence traces and decision rationale for replay/audit
