export function PairingScreen() {
	return (
		<section>
			<h2>Pairing</h2>
			<form>
				<label>
					Daemon URL
					<input name="daemonUrl" />
				</label>
				<label>
					6-digit code
					<input name="code" />
				</label>
				<button type="submit">Pair</button>
			</form>
		</section>
	);
}
