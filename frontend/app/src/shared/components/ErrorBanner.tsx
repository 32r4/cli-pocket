export function ErrorBanner({ message }: { message: string | null }) {
	if (message === null) {
		return null;
	}

	return <div role="alert">{message}</div>;
}
