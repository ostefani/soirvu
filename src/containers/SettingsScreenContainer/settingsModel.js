export function createGeneralModel() {
    return {
        settings: null,
        error: '',
        note: '',
    };
}

export function createSecurityModel() {
    return {
        curPass: '',
        newPass: '',
        newPass2: '',
        note: '',
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
    };
}

export function createExportModel() {
    return {
        pass: '',
        busy: false,
        note: '',
    };
}

export function createImportModel() {
    return {
        busy: false,
        note: '',
    };
}

export function createSyncModel() {
    return {
        info: null,
        busy: false,
        note: '',
        problems: [],
        warnings: [],
        ackAhead: null,
        pruneArm: false,
        restampArm: false,
        restampNote: '',
    };
}
