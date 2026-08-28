The microphone meter under the device picker, at 200×8. It updates at most every 50ms and the fill transition is deliberately 50ms linear — anything slower reads as lag on your own voice.

```jsx
<MeterBar value={rms * 200} />
```
