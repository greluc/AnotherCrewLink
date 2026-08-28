/**
 * The six-character lobby code.
 */
export interface LobbyCodeProps {
  code?: string;
  /** Tint behind the code — the client uses the local player's crew colour. */
  background?: string;
  /** True when "Show Lobby Code" is off: the chip reads LOBBY instead. */
  hidden?: boolean;
}
export function LobbyCode(props: LobbyCodeProps): JSX.Element;
