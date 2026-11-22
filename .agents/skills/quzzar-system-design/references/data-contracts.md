# Schema-backed data design with quzzar-flow

Use this procedure for table infrastructure and structured data pipelines in both new-system
design and existing-system audits. The graph should let the user inspect how a concrete row
shape supplies the rest of the app and how each operation produces another structured shape.

## Establish the row contract

For each table, describe its columns and the JSON representation of one row: field names,
types, nullability, optionality, enums, nested JSON, identifiers, and relevant bounds. Separate
null from missing values. Record SQL-to-JSON representation choices explicitly, including
timestamps, decimals, and large integers. Use actual SQL/query/serialization behavior during
an audit; mark undecided details in a new design rather than inventing them.

Define a reusable Zod contract for that wire shape in the diagram authoring code. The current
quzzar-flow API accepts Zod contracts and describes/checks them through JSON Schema; do not
pass a raw JSON Schema object as a node's `input` or `output`. This TypeScript diagram is a
consumer of the package, even when the application being designed is Rust. Reuse or derive
the diagram contract from the application's authoritative wire contract where practical.
Avoid independently maintained, contradictory Rust and diagram models.

For a table source, declare `input: z.object({})` and the row schema as `output`. Show column
information in table/schema content blocks and document primary keys, foreign keys, unique
constraints, and cardinality separately. Labels and descriptive content do not enforce those
SQL constraints. An empty input is appropriate for a source; it must not be used to erase a
real consumer's requirements.

## Follow the actual payload

For each connection identify the shape that is supplied: a whole row, selected columns, an
optional lookup result, an array, or an aggregate. A single row is not an array of rows.
Represent a scoped query, join, projection, or aggregation as its own processing node when it
changes shape. Declare its arguments and result contract; do not imply that a table-source
arrow performs a database query, authorizes access, or filters by the current user.

A consumer declares only the fields it needs, with those fields required when processing
requires them. Derive a subset with `.pick(...)` or an explicit object contract. `.partial()`
makes fields optional; it does not mean "select some columns." Do not weaken a required
consumer field simply to make an incompatible producer validate.

Describe transformations in `processing` and declare their output schemas. Keep application
logic in the actual implementation: processing descriptions are text, never executable graph
functions. Use explicit connection mappings when paths are renamed, nested differently, or
multiple suppliers could satisfy the same field. Extra producer fields need not be consumed.
Do not silently repair ambiguous or incompatible contracts by choosing another meaning.

## Example: user row to subscription access

This illustrates a table's row contract supplying a narrower consumer contract. The consumer
uses the user's ID and subscription status; the email column remains available upstream but
is not part of the consumed payload. The status values are illustrative, not a prescribed
subscription model.

```ts
import { z } from 'zod';
import { SystemDesigner } from 'quzzar-flow';

const userRow = z.object({
  id: z.string(),
  email: z.string(),
  subscriptionStatus: z.enum(['active', 'inactive']),
});
const subscriptionUser = userRow.pick({ id: true, subscriptionStatus: true });
const sd = new SystemDesigner({ name: 'User subscription data flow' });

sd.add({
  id: 'users',
  name: 'users row',
  input: z.object({}),
  output: userRow,
  processing: 'Expose the JSON contract of one users row, not a query execution.',
  render: [{ type: 'schema', title: 'Row columns', schema: userRow }],
  additionalInfo: 'Intended primary key: id. SQL constraints and RLS require separate checks.',
});
sd.add({
  id: 'subscription-access',
  name: 'Resolve subscription access',
  input: z.object({ user: subscriptionUser }),
  output: z.object({ userId: z.string(), allowed: z.boolean() }),
  processing: 'Read user.id and user.subscriptionStatus; describe the agreed access rule and emit userId and allowed.',
});
sd.connect('users', 'subscription-access', {
  map: [{ from: [], to: ['user'] }],
});

const validation = sd.validate();
console.log(validation.status, validation.bindings, validation.issues);
```

Inspect the destination input and connection bindings in the rendered graph. They should show
`users.id` supplying `user.id` and `users.subscriptionStatus` supplying
`user.subscriptionStatus`. Follow this output into its actual downstream consumers in a real
design; do not stop at isolated table cards. If the runtime sends only a query projection,
show that query result as an intermediate contract rather than claiming it returns the whole row.

## Validate and carry the evidence forward

Run `sd.validate()` after meaningful contract edits. Inspect `status`, `issues`, and `bindings`.
A declared connection makes source fields available; destination fields can be assembled from
multiple suppliers. Explicit mappings are checked too. Confirm each required consumer field
has the intended source and that the output fits the next step's input.

- `valid`: the package proved the checks it supports.
- `invalid`: a mismatch, missing field, ambiguity, or structural issue needs attention.
- `unverified`: unsupported constraints or conversions prevent a proof; explain the gap.

Record validation status and unresolved issues alongside each map/review. Resolve invalid
proposed contracts and explicitly disclose remaining unverified constraints before claiming
contract verification. Preserve actual implementation mismatches in current-state audits.
Custom refinements, coercions, defaults, transforms, and other unsupported constructs must
not be presented as verified simply because Zod can express them.

Illustrative JSON content is not a live payload or automatically validated sample. If using
sample data as evidence, parse it separately with its contract and label it illustrative.
Graph validation does not prove transformation correctness, query filtering, authorization,
foreign keys, cardinality, transaction safety, or runtime behavior. Verify those in the actual
implementation. Never report a schematic user-to-subscription connection as proof of access
control or subscription correctness.

Carry row schemas, projection decisions, transformations, mappings, and validation findings
into `system.md`, `plan.md`, and independent reviews. In the final audit, derive contracts
from actual migrations, queries, serializers, and consumers and compare them with the approved
map. Do not obtain a clean report by relabeling the proposal as implemented behavior.
