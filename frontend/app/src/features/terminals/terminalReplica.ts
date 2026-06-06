interface BufferedLiveOutput {
	seq: number;
	chunk: string;
}

interface LoadedRange {
	startSeq: number | null;
	endSeq: number | null;
}

interface PendingSnapshot {
	terminalId: string;
	snapshot: string;
	startSeq: number | null;
}

const utf8Encoder = new TextEncoder();

export class TerminalReplica {
	private renderedTextCache: string | null = "";
	private prependedHistoryChunks: string[] = [];
	private bodyChunks: string[] = [];
	private loadedRange: LoadedRange = { startSeq: null, endSeq: null };
	private pendingSnapshot: PendingSnapshot | null = null;
	private isRedrawing = false;
	private bufferedLiveOutput: BufferedLiveOutput[] = [];
	private pendingDetachedLiveOutput: BufferedLiveOutput[] = [];

	getLoadedRange() {
		return { ...this.loadedRange };
	}

	isCurrentlyRedrawing() {
		return this.isRedrawing;
	}

	reset() {
		this.renderedTextCache = "";
		this.prependedHistoryChunks = [];
		this.bodyChunks = [];
		this.loadedRange = { startSeq: null, endSeq: null };
		this.pendingSnapshot = null;
		this.isRedrawing = false;
		this.bufferedLiveOutput = [];
		this.pendingDetachedLiveOutput = [];
	}

	queueSnapshot(terminalId: string, snapshot: string, startSeq: number | null) {
		this.pendingSnapshot = {
			terminalId,
			snapshot,
			startSeq,
		};
	}

	consumePendingSnapshot(terminalId: string) {
		if (this.pendingSnapshot?.terminalId !== terminalId) {
			return null;
		}

		const snapshot = this.pendingSnapshot;
		this.pendingSnapshot = null;
		return snapshot;
	}

	hasPendingSnapshot(terminalId: string) {
		return this.pendingSnapshot?.terminalId === terminalId;
	}

	queueDetachedLiveOutput(seq: number, chunk: string) {
		if (chunk.length === 0) {
			return;
		}
		this.pendingDetachedLiveOutput.push({ seq, chunk });
	}

	drainDetachedLiveOutput() {
		const replay = this.pendingDetachedLiveOutput;
		this.pendingDetachedLiveOutput = [];
		return replay;
	}

	beginRedraw() {
		this.isRedrawing = true;
	}

	endRedraw() {
		this.isRedrawing = false;
	}

	drainBufferedLiveOutput() {
		const replay = this.bufferedLiveOutput;
		this.bufferedLiveOutput = [];
		return replay;
	}

	bufferLiveOutput(seq: number, chunk: string) {
		const lastBuffered =
			this.bufferedLiveOutput[this.bufferedLiveOutput.length - 1];
		if (lastBuffered == null || lastBuffered.seq <= seq) {
			this.bufferedLiveOutput.push({ seq, chunk });
			return;
		}

		let insertAt = this.bufferedLiveOutput.length - 1;
		while (insertAt >= 0 && this.bufferedLiveOutput[insertAt].seq > seq) {
			insertAt -= 1;
		}
		this.bufferedLiveOutput.splice(insertAt + 1, 0, { seq, chunk });
	}

	setSnapshotContent(snapshot: string, startSeq: number | null) {
		this.prependedHistoryChunks = [];
		this.bodyChunks = snapshot.length === 0 ? [] : [snapshot];
		this.renderedTextCache = snapshot;
		this.loadedRange = {
			startSeq,
			endSeq: startSeq == null ? null : chunkEndSeq(startSeq, snapshot),
		};
	}

	prependHistoryChunk(chunk: string, startSeq: number) {
		if (chunk.length === 0) {
			this.loadedRange.startSeq = startSeq;
			return;
		}

		this.prependedHistoryChunks.push(chunk);
		this.renderedTextCache = null;
		this.loadedRange.startSeq = startSeq;
	}

	appendLiveChunk(chunk: string, seq: number) {
		if (chunk.length === 0) {
			return;
		}

		this.bodyChunks.push(chunk);
		this.renderedTextCache = null;
		this.loadedRange.endSeq = seq;
		if (this.loadedRange.startSeq == null) {
			this.loadedRange.startSeq = seq - utf8Encoder.encode(chunk).length;
		}
	}

	currentRenderedText() {
		if (this.renderedTextCache != null) {
			return this.renderedTextCache;
		}

		let text = "";
		for (
			let index = this.prependedHistoryChunks.length - 1;
			index >= 0;
			index -= 1
		) {
			text += this.prependedHistoryChunks[index];
		}
		for (const chunk of this.bodyChunks) {
			text += chunk;
		}
		this.renderedTextCache = text;
		return this.renderedTextCache;
	}
}

function chunkEndSeq(startSeq: number, chunk: string) {
	return startSeq + utf8Encoder.encode(chunk).length;
}
