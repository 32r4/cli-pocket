import type { DaemonRecord } from "@/state/daemon-registry/types";

export function DaemonListScreen({ daemons }: { daemons: DaemonRecord[] }) {
	return (
		<section>
			<h2>Daemons</h2>
			<ul>
				{daemons.map((daemon) => (
					<li key={daemon.id}>{daemon.label}</li>
				))}
			</ul>
		</section>
	);
}
