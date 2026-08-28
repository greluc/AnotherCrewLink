The one control on the waiting screen. Use it whenever a screen's job is "the game is not running yet".

```jsx
<LaunchButton label="Steam" platforms={["Steam","Epic Games","Microsoft","Custom"]} onLaunch={open} />
```

Disabled when no platform was detected; the label then reads "No platform detected".
