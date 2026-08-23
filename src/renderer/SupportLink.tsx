import Link from '@mui/material/Link';
import Typography from '@mui/material/Typography';
import type React from 'react';
import { shell, ipcRenderer } from 'electron';
import { makeStyles } from 'tss-react/mui';

const useStyles = makeStyles()(() => ({
	button: {
		color: 'white',
		background: 'none',
		padding: '2px 10px',
		borderRadius: 10,
		border: '2px solid white',
		fontSize: 19,
		outline: 'none',
		fontWeight: 500,
		fontFamily: '"Varela Round", sans-serif',
		marginTop: 24,
		'&:hover': {
			borderColor: '#00ff00',
			cursor: 'pointer',
		},
	},
}));
const onRefreshClick = () => {
	ipcRenderer.send('reload');
};

const SupportLink: React.FC = () => {
	const { classes } = useStyles();

	return (
		<Typography align="center">
			Need help?&nbsp;
			<Link href="#" color="secondary" onClick={() => shell.openExternal('https://discord.gg/4cpvp3KyhF')}>
				Get support
			</Link>
			<button type="button" className={classes.button} onClick={onRefreshClick}>
				Reload
			</button>
		</Typography>
	);
};

export default SupportLink;
