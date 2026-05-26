import type { ReactNode } from "react";

export function Shell({ children }: { children: ReactNode }) {
	return (
		<div>
			<header>
				<h1>cli-pocket</h1>
				<nav aria-label="Primary">
					<a href="#daemons">Daemons</a>
					<a href="#workspace">Workspace</a>
					<a href="#settings">Settings</a>
				</nav>
			</header>
			{children}
		</div>
	);
}
