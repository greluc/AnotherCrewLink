Every setting whose consequence is not obvious raises one of these first: the title is "Are you sure?" and the body is the specific consequence.

```jsx
<Dialog title="Are you sure?" actions={<><Button>Confirm</Button><Button>Cancel</Button></>}>
  This will reset ALL settings to their default values.
</Dialog>
```


**Rust counterpart:** the confirmation a `Change` carries as `warning`. A paint function cannot wait for an answer, so the dialog belongs to the caller.
