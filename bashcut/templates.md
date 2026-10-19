# Project templates with slots (#42)

Reusable starting points for batches and recipes. Code: `crates/engine/src/bashcut/templates.rs`, tests in
`templates_tests.rs`. Built only on upstream commands (`file.saveCopy`, `file.exportFrame`, `file.open`,
`media.replaceFootage`, `file.saveAs`); no upstream edit.

**Bundle** (a folder; by name under `<data dir>/bashcut/templates/<name>/`):

```text
template.json   {"schema":"bashcut-template/1","name","project":"project.fcproj","poster","slots":[{id,name,item,kind,file,original}]}
project.fcproj  a copy of the project
assets/         slotN.<ext>: each slot's own media, used when a fill is missing
poster.png      a program frame (best effort)
```

A slot is a file-based media item: `kind` is `video`, `audio` (music bed) or `image` (logo).

| Command | Params | What it does |
|---|---|---|
| `bashcut.template.save` | `path` or `name`, `slots: [id \| {item, name}]`, `title?`, `poster=true`, `overwrite=false` | writes the bundle |
| `bashcut.template.instantiate` | `template` (folder or name), `fills: [{slot, file}]`, `saveAs?` | opens the template's project, replaces each slot's footage (clips, effects, keyframes, caption positions stay), folds the replacements into one undo step `Instantiate Template`, saves as `saveAs`; never saves over the template |
| `bashcut.template.list` | `dir?` | templates in the data dir with their slots and poster |

`slot` matches a slot id (`slot1`) or its name. A missing fill keeps the template's media (its copy in `assets/`).
Every command takes `dir` to use another templates folder (tests, batch runs). The batch pipeline's `template` step
calls `bashcut.template.instantiate`.
