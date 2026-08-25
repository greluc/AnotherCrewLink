import React, { useEffect, useState } from 'react';
import { styled } from '@mui/material/styles';
import { makeStyles } from 'tss-react/mui';
import Table from '@mui/material/Table';
import TableBody from '@mui/material/TableBody';
import TableCell from '@mui/material/TableCell';
import TableContainer from '@mui/material/TableContainer';
import TableHead from '@mui/material/TableHead';
import TableRow from '@mui/material/TableRow';
import Paper from '@mui/material/Paper';
import Button from '@mui/material/Button';
import { ipcRenderer } from 'electron';
import { IpcMessages } from '../../common/ipc-messages';
import { io, type Socket } from 'socket.io-client';
import i18next from 'i18next';
import { Dialog, DialogActions, DialogContent, DialogContentText, DialogTitle, Tooltip } from '@mui/material';
import languages from '../language/languages';
import type { PublicLobbyMap, PublicLobby } from '../../common/PublicLobby';
import { sortLobbies } from './sortLobbies';
import { modList, type ModsType } from '../../common/Mods';
import { GameState } from '../../common/AmongUsState';
import SettingsStore from '../settings/SettingsStore';

const serverUrl = SettingsStore.get('serverURL', 'http://localhost:9736');
const language = SettingsStore.get('language', 'en');
i18next.changeLanguage(language);

const StyledTableCell = styled(TableCell)(({ theme }) => ({
	'&.MuiTableCell-head': {
		backgroundColor: '#1d1a23',
		color: theme.palette.common.white,
	},
	'&.MuiTableCell-body': {
		fontSize: 14,
	},
}));

const StyledTableRow = styled(TableRow)({
	'&:nth-of-type(odd)': {
		backgroundColor: '#25232a',
	},
	'&:nth-of-type(even)': {
		backgroundColor: '#1d1a23',
	},
});

const useStyles = makeStyles()({
	table: {
		minWidth: 700,
	},
	container: {
		// Was a fixed 400px in a window that is resizable, so growing the window only
		// added empty space below the table.
		maxHeight: 'calc(100vh - 130px)',
	},
});

function getModName(mod: string): string {
	return modList.find((o) => o.id === mod)?.label || (mod ?? 'None');
}

interface LobbyBrowserProps {
	/** Injected by withTranslation() in the container. */
	t: (key: string) => string;
}

export default function LobbyBrowser({ t }: LobbyBrowserProps) {
	const { classes } = useStyles();
	const [publiclobbies, setPublicLobbies] = useState<PublicLobbyMap>({});
	const [socket, setSocket] = useState<Socket>();
	const [code, setCode] = React.useState('');
	const [, forceRender] = useState({});

	const [mod, setMod] = useState<ModsType>('NONE');

	useEffect(() => {
		ipcRenderer.invoke(IpcMessages.REQUEST_MOD).then((mod: ModsType) => setMod(mod));

		const s = io(serverUrl, {
			transports: ['websocket'],
		});
		setSocket(s);

		s.on('update_lobby', (lobby: PublicLobby) => {
			setPublicLobbies((old) => ({ ...old, [lobby.id]: lobby }));
		});

		s.on('new_lobbies', (lobbies: PublicLobby[]) => {
			setPublicLobbies((old) => {
				const lobbyMap: PublicLobbyMap = { ...old };
				for (const index in lobbies) {
					lobbyMap[lobbies[index].id] = lobbies[index];
				}
				return lobbyMap;
			});
		});
		s.on('remove_lobby', (lobbyId: number) => {
			setPublicLobbies((old) => {
				delete old[lobbyId];
				return { ...old };
			});
		});
		s.on('connect', () => {
			s.emit('lobbybrowser', true);
		});
		const secondPassed = setInterval(() => {
			forceRender({});
		}, 1000);
		return () => {
			// Must be `s`, not the `socket` state: this closure captures the value from
			// the first render, which is still undefined, so the socket was never closed.
			s.emit('lobbybrowser', false);
			s.close();
			clearInterval(secondPassed);
		};
	}, []);

	return (
		<div style={{ height: '100%', width: '100%', paddingTop: '15px' }}>
			<div style={{ height: '100%', boxSizing: 'border-box', padding: '20px' }}>
				<b>{t('lobbybrowser.header')}</b>
				<Dialog
					open={code !== ''}
					// TransitionComponent={Transition}
					keepMounted
					aria-labelledby="alert-dialog-slide-title"
					aria-describedby="alert-dialog-slide-description"
				>
					<DialogTitle id="alert-dialog-slide-title">Lobby information</DialogTitle>
					<DialogContent>
						<DialogContentText id="alert-dialog-slide-description">
							{code.split('\n').map((line, index) => (
								// Static dialog text, so pairing the index with the line is a stable key.
								<div key={`${index}-${line}`}>{line}</div>
							))}
						</DialogContentText>
					</DialogContent>
					<DialogActions>
						<Button onClick={() => setCode('')} color="primary">
							{t('buttons.close')}
						</Button>
					</DialogActions>
				</Dialog>
				<Paper>
					<TableContainer component={Paper} className={classes.container}>
						<Table className={classes.table} aria-label="customized table" stickyHeader>
							<TableHead>
								<TableRow>
									<StyledTableCell>{t('lobbybrowser.list.title')}</StyledTableCell>
									<StyledTableCell align="left">{t('lobbybrowser.list.host')}</StyledTableCell>
									<StyledTableCell align="left">{t('lobbybrowser.list.players')}</StyledTableCell>
									<StyledTableCell align="left">{t('lobbybrowser.list.mods')}</StyledTableCell>
									<StyledTableCell align="left">{t('lobbybrowser.list.language')}</StyledTableCell>
									<StyledTableCell align="left">Status</StyledTableCell>
									{/* {t('lobbybrowser.list.staut')} */}
									<StyledTableCell align="left"></StyledTableCell>
								</TableRow>
							</TableHead>
							<TableBody>
								{Object.values(publiclobbies)
									.sort(sortLobbies)
									.map((row: PublicLobby) => (
										<StyledTableRow key={row.id}>
											<StyledTableCell component="th" scope="row">
												{row.title}
											</StyledTableCell>
											<StyledTableCell align="left">{row.host}</StyledTableCell>
											<StyledTableCell align="left">
												{row.current_players}/{row.max_players}
											</StyledTableCell>
											<StyledTableCell align="left">{getModName(row.mods)}</StyledTableCell>
											<StyledTableCell align="left">
												{(languages as Record<string, { name?: string }>)[row.language]?.name ?? 'English'}
											</StyledTableCell>
											<StyledTableCell align="left">
												{row.gameState === GameState.LOBBY ? 'Lobby' : 'In game'}{' '}
												{row.stateTime && new Date(Date.now() - row.stateTime).toISOString().substr(14, 5)}
											</StyledTableCell>
											<StyledTableCell align="right">
												<Tooltip
													title={
														row.gameState !== GameState.LOBBY
															? t('lobbybrowser.code_tooltips.in_progress')
															: row.max_players === row.current_players
																? t('lobbybrowser.code_tooltips.full_lobby')
																: row.mods != mod
																	? `${t('lobbybrowser.code_tooltips.incompatible')} '${getModName(mod)}' ${t(
																			'lobbybrowser.code_tooltips.and'
																		)} '${getModName(row.mods)}'`
																	: ''
													}
												>
													<span>
														<Button
															disabled={
																row.gameState !== GameState.LOBBY ||
																row.max_players === row.current_players ||
																row.mods != mod
															}
															variant="contained"
															color="secondary"
															onClick={() => {
																socket?.emit(
																	'join_lobby',
																	row.id,
																	(state: number, codeOrError: string, server: string, _publicLobby: PublicLobby) => {
																		if (state === 0) {
																			setCode(`${t('lobbybrowser.code')}: ${codeOrError} \n Region: ${server}`);
																			// This once asked the main process to write the lobby code
																			// into the game so it joined by itself. The write path was
																			// removed on 2026-08-24 and the code is shown instead.
																		} else {
																			setCode(`Error: ${codeOrError}`);
																		}
																	}
																);
															}}
														>
															Show code
														</Button>
													</span>
												</Tooltip>
												{/* <Button variant="contained" color="secondary" style={{ marginLeft: '5px' }}>
												report
											</Button> */}
											</StyledTableCell>
										</StyledTableRow>
									))}
							</TableBody>
						</Table>
					</TableContainer>
				</Paper>
			</div>
		</div>
	);
}
