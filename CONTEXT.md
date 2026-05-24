# Context

## Repository Rules

- No AI artifact may be committed to the repository.
- All AI artifacts must be created inside `/artifacts`.
- The `/artifacts` directory must remain ignored by Git.
- All branches must be named in English.
- All commits must be written in English and follow Conventional Commits.
- The entire repository must be maintained in English.

## Testing

- Every core `gitrex` operation must have tests.
- Any change to a core operation must be mirrored in the tests.
- Test updates are required whenever the behavior changes.

## Documentation

- Important changes must be reflected in the project documentation.
- If a workflow, command, or rule changes, update the relevant docs in the same change.

## Notes

- Keep AI-generated work isolated in `/artifacts` until it is explicitly reviewed for inclusion.
- Branch history means the commit list and graph for one branch ref, not the whole repo history.
