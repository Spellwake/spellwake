# Build from the database outward

Apply this order to new systems and changes to existing systems after the design and plan
have been reviewed. It orders implementation, not discovery: map the whole agreed system
and its contracts before building. Skip layers the change does not touch. Do not invent
tables, endpoints, edge functions, microservices, or wrapper frameworks to fill the sequence.

## Plan concrete stages

Organize `plan.md` in the order below. Give each stage its actual changes, dependencies,
checks, and observable exit condition. Mark a stage complete only with supporting evidence.
Finish and verify the affected foundation before building its consumers. If a check fails,
fix that stage instead of building more layers over it. Keep design questions separate from
implementation tasks; return to the user only for decisions that change the approved design.

1. **Database structures.** Create the required tables and columns first, along with keys,
   constraints, indexes, and required reference records. Implement the row shapes already
   agreed in the schema-backed graph. Verify migrations on a fresh database and, where live
   data exists, on a representative existing schema/data state. Distinguish missing fields
   from nullable ones and confirm serialization matches the wire contract. Keep new tables
   inaccessible until appropriate policies/grants are in place; schema and access-control
   changes may belong to the same deployment migration.
2. **RLS and permissions.** Implement the access policies and required grants next. Test the
   actual roles and identity path: anonymous, owner, non-owner, cross-tenant, and elevated
   operations where applicable. Cover allowed and denied reads/inserts/updates/deletes and
   attempted ownership/tenant reassignment. Do not run all access tests as service role and
   mistake a bypass for RLS verification. Resolve authorization failures before endpoints.
3. **Wire contracts and API/edge endpoints.** Implement the approved request/response schemas
   and API or edge-function handlers against the database foundation. Preserve the standard
   success/fail/error response shape. Test valid requests, invalid input, authorization,
   missing/empty results, and the agreed failure and boundary cases. Exercise overload,
   concurrency, and limits where relevant to this system; do not invent a load requirement.
4. **Required services and workers.** Implement the microservices, jobs, and processing the
   endpoints need. Prefer the shared Rust domain implementation over duplicate logic. Verify
   service contracts and behavior, then run endpoint-to-service integration checks. Define
   endpoint contracts first; if a handler depends on a service, build that dependency before
   claiming the handler works. Do not use a stubbed success or a mock-only test as evidence
   that the real endpoint/service path is complete.
5. **Typed client functions.** Build the shared client API that the UI will call. Functions
   or a typed command map must require the correct parameters and expose the structured
   response for each operation. Validate external responses at runtime and handle fail/error
   distinctly. Reuse the same contracts and browser/Tauri host adapter rather than scattering
   raw requests through components. Test serialization, response parsing, and failure behavior
   against the actual boundary. Keep this layer small; no new generic framework is required.
6. **UI integration.** Wire the approved controls and user actions into those client functions.
   Verify that a click supplies the intended parameters, invokes the intended operation, and
   handles the agreed success/loading/error states. Use rendered and interaction checks for
   the changed surface. Detailed UI/UX design follows `quzzar-ui-ux-design`: obtain approved screen/state images
   and flow coverage before UI implementation. Design can precede backend implementation;
   wire the UI to verified client functions at this stage. Do not expand the approved UI.

Keep checks proportional to each stage's real invariants. Reuse passing evidence until changes
invalidate it; do not restart broad investigation or add speculative systems after a stage has
met its exit condition. After the affected stages pass, perform the workflow's final system
review against the approved map, requirements, and checklist.

## Live data: expand, release, migrate, contract

For live-data changes, default to an additive rollout. Decide the actual migration and any
period of backward compatibility during planning. Do not drop, rename destructively, or
repurpose columns that deployed code or existing records still depend on while building
the replacement.

Keep two explicit sections in `plan.md`:

- **Build and rollout:** additive tables/columns/constraints, RLS, endpoints, services, client
  functions, UI integration, stage checks, and the authorized release sequence.
- **After-live migration and removal:** exact source/destination fields, record migration,
  verification, cutover dependencies, and every superseded column/table/code path to delete.

The second section is required whenever removals are deferred. Record what makes each item
safe to run, how to verify it, and its remaining status. Put a durable follow-up in the repo's
maintained migration/deployment record or issue tracker so an ignored working checklist is
not the only record. Do not create a new tracking system when one already exists.

Use the following sequence, adjusted to the agreed data and deployment requirements:

1. **Expand.** Add the new structures while the existing deployed path remains usable. Add
   compatible constraints and authorization without exposing unprotected data. Plan database
   locks, table rewrites, index creation, and large-data operations against the actual database;
   additive SQL alone does not guarantee an interruption-free deployment.
2. **Build and release the new path.** Complete and verify the layers above, then promote
   through dev, staging, and main under the project's deployment process and existing
   authorization. Building migration files does not authorize running them in production.
   Keep the old structures until the running application no longer needs them. Where rollout
   overlap requires temporary read/write compatibility, specify its scope and removal condition
   explicitly; do not silently introduce dual writes or fallback behavior.
3. **Migrate and reconcile.** Once the new path is live and the agreed prerequisites hold,
   run the authorized data migration. Address records changed during rollout, resumability,
   consistency, and recovery as applicable. Verify counts plus relevant values, relationships,
   constraints, and behavior; a completed script or matching row counts alone do not prove
   correct migration. If a specific feature needs data before activation, record that
   dependency and migrate before activating it rather than making the live path unusable.
4. **Contract and delete.** Confirm deployed consumers and the agreed rollback window no
   longer depend on the old structures. Remove superseded columns, tables, code, compatibility
   paths, tests, exports, and configuration in a separate reviewed cleanup change. Verify the
   resulting schema and application. Temporary compatibility has an explicit end; it is not
   permission to retain dead code indefinitely.

New systems without live data do not need an invented compatibility period or after-live
migration. Create and verify their approved structures directly. In either case, report the
build, release, migration, and removal status separately. Do not call a post-live migration
or cleanup complete because the new feature builds or its rollout is ready. Continue all
currently authorized work; preserve future-environment dependencies as explicit pending work.
