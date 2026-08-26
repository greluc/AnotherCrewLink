# The hat collection, as it is served

`hats.json`, fetched on 2026-08-26 from the commit `acl_types::cosmetics::HAT_COLLECTION_URL`
pins:

```
https://cdn.jsdelivr.net/gh/greluc/AnotherCrewLink-Hats@14bb0cb592a23d2cee25a0c368506446abadaad8/hats.json
```

Vendored for the same reason `test/fixtures/offsets` is: the parser's job is to accept the
real file, and a sample invented alongside the parser only proves the parser agrees with
itself. The pin is a commit, so this copy cannot drift from what players download without
the pin moving — and if the pin moves, this file moves with it or
`the_shipped_pin_still_serves_this_file` starts describing a tree nobody has.

**What is in it**, and each number is a test in `acl_ui::hats`:

| | |
| --- | --- |
| Collections | 1 — `NONE`. The four mod collections upstream ships went with the third-party artwork. |
| Entries | 983 — 584 hats, 215 skins, 184 visors |
| With no `image` | 46. They draw nothing, in both clients. |
| With a `back_image` | 158 |
| With `multi_color` | 30 |
| With their own geometry | 0. Every entry takes the collection's defaults. |
