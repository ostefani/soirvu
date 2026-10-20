import { EVENT_TYPE } from '@/constants.js';

export function createGeneralModel() {
    return {
        settings: null,
        error: '',
        note: '',
        noteEvent: EVENT_TYPE.SAVED,
    };
}

export function createSecurityModel() {
    return {
        curPass: '',
        newPass: '',
        newPass2: '',
        note: '',
        noteEvent: EVENT_TYPE.UPDATED,
        newRecoveryKey: '',
        busy: false,
        rotated: false,
        rotating: false,
    };
}

export function createTouchIdModel() {
    return {
        status: null,
        busy: false,
        note: '',
        noteEvent: EVENT_TYPE.UPDATED,
    };
}

export function createExportModel() {
    return {
        pass: '',
        busy: false,
        note: '',
        noteEvent: EVENT_TYPE.SAVED,
    };
}

export function createImportModel() {
    return {
        busy: false,
        note: '',
        noteEvent: EVENT_TYPE.SAVED,
    };
}

export function createSyncModel() {
    return {
        info: null,
        busy: false,
        note: '',
        noteEvent: EVENT_TYPE.INFO,
        problems: [],
        warnings: [],
        ackAhead: null,
        pruneArm: false,
        restampArm: false,
        restampNote: '',
        restampNoteEvent: EVENT_TYPE.UPDATED,
    };
}
