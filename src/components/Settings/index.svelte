<script>
    import Button from '@/components/Button/index.svelte';
    import Input from '@/components/Input/index.svelte';
    import Select from '@/components/Select/index.svelte';
    import ErrorComponent from '@/components/ErrorComponent/index.svelte';
    import NoteComponent from '@/components/NoteComponent/index.svelte';
    import WarningComponent from '@/components/WarningComponent/index.svelte';
    import Tabs from '@/components/Tabs/index.svelte';
    import Checkbox from '@/components/Checkbox/index.svelte';
    import Description from '@/components/Description/index.svelte';
    import SettingsCard from '@/components/SettingsCard/index.svelte';
    import CardDivider from '@/components/CardDivider/index.svelte';
    import TabPanel from '@/components/TabPanel/index.svelte';
    import ActionRow from '@/components/ActionRow/index.svelte';
    import LocationField from '@/components/LocationField/index.svelte';
    import FieldGrid from '@/components/FieldGrid/index.svelte';
    import InlineProgress from '@/components/InlineProgress/index.svelte';
    import SyncPeers from '@/components/SyncPeers/index.svelte';
    import { provideActions } from '@/components/ActionRow/context.js';
    import {
        TABS,
        DEFAULT_TEXT,
        PRIVACY_OPTIONS,
        SECURITY_SECOND_COLUMN,
        ACTION_KINDS,
        withDefaults,
    } from './content.js';
    import { getTouchIdView, isTouchIdOffered } from './touch-id.js';

    let {
        general,
        security,
        exp,
        touchId,
        imp,
        sync,
        settingsText,
        fields,
        ACTIONS,
        actions,
        disabled,
        handlers,
        setSecurityField,
        setExportPass,
        setTimeoutMin,
        onTabChange,
    } = $props();

    let activeTab = $state('general');

    function changeTab(id) {
        onTabChange?.(id);
        activeTab = id;
    }

    // Every ActionRow below reads ACTIONS / handlers / disabled from here.
    provideActions({
        get ACTIONS() {
            return ACTIONS;
        },
        get handlers() {
            return handlers;
        },
        get disabled() {
            return disabled;
        },
    });

    const text = $derived(withDefaults(DEFAULT_TEXT, settingsText));

    const privacyOptions = $derived(
        PRIVACY_OPTIONS.filter((option) => !option.platform || option.platform === general.settings?.platform),
    );

    const touchIdView = $derived(
        isTouchIdOffered(touchId.status) ? getTouchIdView(touchId.status, text) : null,
    );
</script>

<div class="settings-wrapper">
    <Tabs tabs={TABS} {activeTab} onChange={changeTab} />

    {#if general.settings}
        <!-- ================================================================= -->
        <!-- TAB 1: GENERAL                                                    -->
        <!-- ================================================================= -->
        {#if activeTab === 'general'}
            <TabPanel id="general">
                <SettingsCard title={text.storageTitle} notes={[text.storageIntro]}>
                    <LocationField label={text.vaultLocation} value={general.settings.vault_location} />

                    <CardDivider />

                    <div class="timeout-row">
                        <Input
                            field={fields.timeout}
                            value={general.timeoutMin}
                            oninput={(e) => setTimeoutMin(e.currentTarget.value)} />
                        <ActionRow names={actions.timeout} />
                    </div>
                </SettingsCard>

                <SettingsCard title={text.privacyTitle} notes={[text.privacyIntro]}>
                    <div class="checkbox-group">
                        {#each privacyOptions as option (option.setting)}
                            <Checkbox
                                checked={general.settings[option.setting]}
                                onchange={(e) => handlers[option.handler](e.currentTarget.checked)}
                                label={text[option.label]} />
                        {/each}
                    </div>
                </SettingsCard>

                {#if general.error}
                    <ErrorComponent text={general.error} />
                {/if}
            </TabPanel>

            <!-- ================================================================= -->
            <!-- TAB 2: SECURITY                                                   -->
            <!-- ================================================================= -->
        {:else if activeTab === 'security'}
            <TabPanel id="security">
                <SettingsCard title={text.securityTitle} notes={text.securityNotes}>
                    <FieldGrid
                        fields={fields.security}
                        getValue={(field) => security[field.name]}
                        onInput={(field, value) => setSecurityField(field.name, value)}
                        secondColumn={SECURITY_SECOND_COLUMN}>
                        <ActionRow names={actions.security} kinds={ACTION_KINDS} />
                        <ActionRow names={actions.rotate} separated />

                        {#if security.rotating}
                            <InlineProgress text={text.rotating} />
                        {/if}

                        {#if security.newRecoveryKey}
                            <NoteComponent variant="warning" title={text.newKeyLead} text={text.newKeyNote}>
                                <p><code>{security.newRecoveryKey}</code></p>
                            </NoteComponent>
                        {/if}
                    </FieldGrid>
                </SettingsCard>

                {#if touchIdView}
                    <SettingsCard title={text.touchIdTitle}>
                        {#if touchIdView.note}
                            <NoteComponent variant="warning" text={touchIdView.note} />
                        {/if}
                        {#if touchIdView.description}
                            <Description text={touchIdView.description} />
                        {/if}
                        {#if touchIdView.action}
                            <ActionRow>
                                <Button
                                    label={touchIdView.action.label}
                                    kind={touchIdView.action.kind}
                                    handler={handlers[touchIdView.action.handler]}
                                    disabled={touchId.busy} />
                            </ActionRow>
                        {/if}
                    </SettingsCard>
                {/if}
            </TabPanel>

            <!-- ================================================================= -->
            <!-- TAB 3: BACKUPS                                                    -->
            <!-- ================================================================= -->
        {:else if activeTab === 'backup'}
            <TabPanel id="backup">
                <SettingsCard title={text.backupTitle} notes={text.backupNotes}>
                    <LocationField
                        label={text.backupLocation}
                        value={general.settings.backup_location}
                        emptyText={text.notSet}>
                        <ActionRow names={actions.backupFolder} />
                    </LocationField>

                    <CardDivider />

                    <div class="schedule-field">
                        <Select
                            field={fields.schedule}
                            value={general.settings.backup_schedule}
                            onchange={(e) => handlers.saveSchedule(e.currentTarget.value)} />
                    </div>

                    <ActionRow names={actions.backupNow} />
                    {#if general.error}<ErrorComponent text={general.error} />{/if}
                </SettingsCard>
            </TabPanel>

            <!-- ================================================================= -->
            <!-- TAB 4: SYNC                                                       -->
            <!-- ================================================================= -->
        {:else if activeTab === 'sync'}
            <TabPanel id="sync">
                <SettingsCard title={text.syncTitle} notes={[text.syncIntro]}>
                    <LocationField
                        label={text.syncFolder}
                        value={general.settings.sync_location}
                        emptyText={text.notSet}>
                        <ActionRow names={actions.syncFolder} />
                    </LocationField>

                    {#if general.settings.sync_location && sync.info}
                        <CardDivider />

                        {#if sync.info.ack_ahead_seen}
                            <ErrorComponent text={text.ackAhead}>
                                <Button
                                    label={text.dismiss}
                                    kind="texture-button"
                                    handler={handlers.dismissAckAhead}
                                    disabled={sync.busy} />
                            </ErrorComponent>
                        {/if}

                        <SyncPeers deviceId={sync.info.device_id} peers={sync.info.peers} />

                        <!-- kind={null}: sync operations keep the kind from ACTIONS -->
                        <ActionRow names={actions.syncOps} kind={null} />

                        {#if sync.pruneArm}
                            {#each text.pruneNotes as note}
                                <NoteComponent variant="warning" text={note} />
                            {/each}
                        {/if}
                    {/if}

                    {#each sync.problems as problem}
                        <ErrorComponent text={problem} />
                    {/each}
                    {#each sync.warnings as warning}
                        <WarningComponent text={warning} />
                    {/each}
                    {#if sync.ackAhead && !sync.info?.ack_ahead_seen}
                        <ErrorComponent text={sync.ackAhead} />
                    {/if}
                </SettingsCard>

                <SettingsCard title={text.deviceIdentityTitle} notes={[text.restampIntro]}>
                    <ActionRow names={actions.restamp} />
                </SettingsCard>
            </TabPanel>

            <!-- ================================================================= -->
            <!-- TAB 5: IMPORT & EXPORT                                            -->
            <!-- ================================================================= -->
        {:else if activeTab === 'transfer'}
            <TabPanel id="transfer">
                <SettingsCard title={text.exportTitle} notes={[text.exportIntro]} hints={text.exportNotes}>
                    <FieldGrid
                        fields={[fields.exportPass]}
                        getValue={() => exp.pass}
                        onInput={(_field, value) => setExportPass(value)}>
                        <ActionRow names={actions.export} />
                    </FieldGrid>
                </SettingsCard>

                <SettingsCard title={text.importTitle} notes={[text.importIntro]}>
                    <ActionRow names={actions.import} />
                </SettingsCard>
            </TabPanel>
        {/if}
    {:else if !general.error}
        <NoteComponent text={text.loading} />
    {/if}
</div>

<style src="./styles.css"></style>
