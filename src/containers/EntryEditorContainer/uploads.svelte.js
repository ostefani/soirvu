import { createJobQueue } from '../../lib/job-queue.svelte.js';
import { mayFinishAfterFlush } from '../../lib/entry-commit.js';
import { ATTACH_CHUNK_SIZE, MAX_CONCURRENT_UPLOADS } from './constants.js';
import { SESSION_TEXT } from './data.js';
import { attachError, attachmentStatus, uploadFailureText } from './labels.js';

export function createUploads({ api, autosave, canUpload, entryId, commit, onAdded, onError }) {
    const queue = createJobQueue({ concurrency: MAX_CONCURRENT_UPLOADS, run: upload });
    const abortRemote = (uploadId) => api.attachmentAbort(uploadId).catch(() => {});

    async function upload(file) {
        let uploadId = null;
        try {
            uploadId = await api.attachmentBegin(entryId(), file.name, file.type || null);
            for (let offset = 0; offset < file.size; offset += ATTACH_CHUNK_SIZE) {
                if (queue.aborted) {
                    await abortRemote(uploadId);
                    return null;
                }
                const chunk = await file.slice(offset, offset + ATTACH_CHUNK_SIZE).arrayBuffer();
                await api.attachmentChunk(uploadId, new Uint8Array(chunk));
            }
            const saved = await autosave.flush();
            if (!mayFinishAfterFlush(saved, autosave.state)) {
                await abortRemote(uploadId);
                uploadId = null;
                return attachError(file.name, SESSION_TEXT.staleAttach);
            }
            const added = await commit((base) => api.attachmentFinish(uploadId, base));
            uploadId = null;
            onAdded({ id: added.id, mime: added.mime, name: file.name, rev: added.rev });
            return null;
        } catch (error) {
            if (uploadId) await abortRemote(uploadId);
            onError(error);
            return uploadFailureText(file.name, error);
        }
    }

    return {
        attach(files) {
            if (canUpload()) queue.add(files);
        },
        abort: queue.abort,
        resume: queue.resume,
        settle: queue.settle,
        get pending() {
            return queue.pending;
        },
        get status() {
            return attachmentStatus(queue.pending, queue.failures);
        },
    };
}
