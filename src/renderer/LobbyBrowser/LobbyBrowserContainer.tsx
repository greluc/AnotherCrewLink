import React from 'react';
import { createRoot } from 'react-dom/client';
import { ThemeProvider, StyledEngineProvider } from '@mui/material/styles';
import { makeStyles } from 'tss-react/mui';
import RefreshSharpIcon from '@mui/icons-material/RefreshSharp';
import CloseIcon from '@mui/icons-material/Close';
import MinimizeIcon from '@mui/icons-material/Minimize';
import IconButton from '@mui/material/IconButton';
import '../css/index.css';
import '@fontsource/source-code-pro/500.css';
import '@fontsource/varela-round/400.css';
import '../language/i18n';
import theme from '../theme';
import LobbyBrowser from './LobbyBrowser';
import { withTranslation } from 'react-i18next';
import { ipcRenderer } from 'electron';

const useStyles = makeStyles()(() => ({
	root: {
		position: 'absolute',
		width: '100vw',
		height: theme.spacing(3),
		backgroundColor: '#1d1a23',
		top: 0,
		WebkitAppRegion: 'drag',
	},
	title: {
		width: '100%',
		textAlign: 'center',
		display: 'block',
		height: theme.spacing(3),
		lineHeight: theme.spacing(3),
		color: theme.palette.primary.main,
	},
	button: {
		WebkitAppRegion: 'no-drag',
		marginLeft: 'auto',
		padding: 0,
		position: 'absolute',
		top: 0,
	},
	minimalizeIcon: {
		'& svg': {
			paddingBottom: '7px',
			marginTop: '-8px',
		},
	},
}));

const TitleBar = function () {
	const { classes } = useStyles();
	return (
		<div className={classes.root}>
			<span className={classes.title} style={{ marginLeft: 10 }}>
				LobbyBrowser
			</span>
			<IconButton className={classes.button} size="small" onClick={() => ipcRenderer.send('reload', true)}>
				<RefreshSharpIcon htmlColor="#777" />
			</IconButton>
			<IconButton
				className={[classes.button, classes.minimalizeIcon].join(' ')}
				style={{ right: 20 }}
				size="small"
				onClick={() => ipcRenderer.send('minimize', true)}
			>
				<MinimizeIcon htmlColor="#777" y="100" />
			</IconButton>

			<IconButton
				className={classes.button}
				style={{ right: 0 }}
				size="small"
				onClick={() => {
					window.close();
				}}
			>
				<CloseIcon htmlColor="#777" />
			</IconButton>
		</div>
	);
};

interface AppProps {
	t: (key: string) => string;
}

export default function App({ t }: AppProps): React.JSX.Element {
	return (
		<StyledEngineProvider injectFirst>
			<ThemeProvider theme={theme}>
				<TitleBar />
				<LobbyBrowser t={t}></LobbyBrowser>
			</ThemeProvider>
		</StyledEngineProvider>
	);
}
// @ts-ignore
const App2 = withTranslation()(App);
// @ts-ignore
// ReactDOM.render was removed in React 19.
const container = document.getElementById('app');
if (container) {
	createRoot(container).render(<App2 />);
}
