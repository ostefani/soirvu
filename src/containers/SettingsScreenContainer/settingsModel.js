export function createGeneralModel() {
    return { settings: null, error: '' };
}

export function createSecurityModel() {
    return {
        curPass: '',
        newPass: '',
        newPass2: '',
        newRecoveryKey: '',
        busy: false,
        rotated: false,
        rotating: false,
    };
}

export function createTouchIdModel() {
    return { status: null, busy: false };
}

export function createExportModel() {
    return { pass: '', busy: false };
}

export function createImportModel() {
    return { busy: false };
}

export function createSyncModel() {
    return {
        info: null,
        busy: false,
        problems: [],
        warnings: [],
        ackAhead: null,
        pruneArm: false,
        restampArm: false,
    };
}
