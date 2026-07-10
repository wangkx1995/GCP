# Issue tracker: Local Markdown

Issues and PRDs for this repo live as markdown files in `.scratch/`.

## Conventions

- One feature per directory: `.scratch/<feature-slug>/`
- The PRD is `.scratch/<feature-slug>/PRD.md`
- Implementation issues are `.scratch/<feature-slug>/issues/<NN>-<slug>.md`, numbered from `01`
- Triage state is recorded as a `Status:` line near the top of each issue file
- Comments and conversation history append under a `## Comments` heading

## Publishing and fetching

When a skill says "publish to the issue tracker", create a file under `.scratch/<feature-slug>/`, creating the directory if needed.

When a skill says "fetch the relevant ticket", read the referenced issue path. The user will normally provide the path or issue number.

## Wayfinding operations

- Map: `.scratch/<effort>/map.md`
- Child ticket: `.scratch/<effort>/issues/NN-<slug>.md`
- Ticket metadata uses `Type:`, `Status:`, and `Blocked by:` lines
- Claim work by setting `Status: claimed` before starting
- Resolve work by adding an `## Answer`, setting `Status: resolved`, and updating the map
