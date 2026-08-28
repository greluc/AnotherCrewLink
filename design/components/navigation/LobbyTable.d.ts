/**
 * The public lobby list.
 */
export interface LobbyTableProps {
  /** Column keys, used as both header labels and row lookups. */
  columns?: string[];
  rows?: Array<Record<string, unknown> & { id?: string | number }>;
  /** Renders the trailing action cell, e.g. a "Show code" button. */
  renderAction?: (row: Record<string, unknown>) => JSX.Element;
}
export function LobbyTable(props: LobbyTableProps): JSX.Element;
