type QueueWaiter<T> = {
	resolve: (result: IteratorResult<T>) => void;
	reject: (error: unknown) => void;
};

export class AsyncValueQueue<T> {
	private readonly values: T[] = [];
	private waiters: QueueWaiter<T>[] = [];
	private closed = false;
	private failure: Error | null = null;

	async next(): Promise<IteratorResult<T>> {
		const value = this.values.shift();
		if (value !== undefined) {
			return { value, done: false };
		}
		if (this.failure != null) {
			throw this.failure;
		}
		if (this.closed) {
			return { value: undefined, done: true };
		}

		return await new Promise<IteratorResult<T>>((resolve, reject) => {
			this.waiters = [...this.waiters, { resolve, reject }];
		});
	}

	push(value: T) {
		if (this.closed || this.failure != null) {
			return;
		}

		const waiter = this.waiters.shift();
		if (waiter != null) {
			waiter.resolve({ value, done: false });
			return;
		}

		this.values.push(value);
	}

	close() {
		if (this.closed || this.failure != null) {
			return;
		}

		this.closed = true;
		for (const waiter of this.waiters) {
			waiter.resolve({ value: undefined, done: true });
		}
		this.waiters = [];
	}

	fail(error: unknown) {
		if (this.closed || this.failure != null) {
			return;
		}

		this.failure = error instanceof Error ? error : new Error("stream failed");
		for (const waiter of this.waiters) {
			waiter.reject(this.failure);
		}
		this.waiters = [];
	}
}
