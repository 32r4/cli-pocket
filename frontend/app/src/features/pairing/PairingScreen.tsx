export function PairingScreen() {
	return (
		<section>
			<h2>Pairing</h2>
			<form>
				<label>
					Pairing link
					<input name="pairUrl" />
				</label>
				<button type="submit">Import</button>
			</form>
		</section>
	);
}
