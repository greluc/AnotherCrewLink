Use for mutually exclusive modes. Do not use for two options — the client uses a checkbox there.

```jsx
<RadioOption label="Push to Talk" value={1} selected={mode === 1} onSelect={setMode} />
```
