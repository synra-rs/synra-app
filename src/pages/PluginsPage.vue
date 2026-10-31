<template>
  <!--
    A workspace page: rows carry a name, a state, an action set and a "more" menu, and the install/registry forms sit
    below them. It takes the shell's full workspace width ([03 §3](../../design-docs/ui-design/03-shell-and-navigation.md)
    puts long-list workspaces at about 1200px) rather than the 768px reading measure the shell used to impose on it.
  -->
  <section class="mx-auto max-w-[1200px] min-w-0 stack gap-6 pb-4" data-page-width="workspace">
    <PageHeader :title="messages.plugins.title">
      <!--
        Trust management has its own entry ([07 §1](../../design-docs/ui-design/07-plugins.md)): it used to be a panel
        below the installed list, so every visit to one's own plugins ended in a long list of signing keys. The label
        carries the count of keys awaiting a decision, because the kernel keeps a plugin running after its key is revoked
        and nothing on the rows can say so yet — a collapsed manager must not be how that goes unnoticed.
      -->
      <AButton data-testid="publishers-toggle" :aria-expanded="publishersExpanded" aria-controls="plugin-publishers" @click="publishersExpanded = !publishersExpanded">
        {{ messages.plugins.publishers.manage(publishersNeedingDecision) }}
      </AButton>
      <AButton ref="installToggle" type="primary" data-testid="install-toggle" :aria-expanded="installExpanded" aria-controls="plugin-install" @click="installExpanded = !installExpanded">
        {{ messages.plugins.installHeading }}
      </AButton>
    </PageHeader>

    <!--
      One entry point, two questions inside it ([07 §1](../../design-docs/ui-design/07-plugins.md)): "can my plugins run"
      and "what could I install". The install button stays in the header because installing is a page-level action, not
      a property of either list.
    -->
    <ASegmented v-model:value="pluginCategory" :options="pluginCategoryOptions" data-testid="plugin-category" />

    <!--
      Trust management, opened from its own header button rather than sitting under whichever list was last scrolled to
      ([07 §1](../../design-docs/ui-design/07-plugins.md)). `v-show` keeps a pending decision open across a category
      switch, and the button's label carries the count so a collapsed manager cannot be the reason nobody noticed.
    -->
    <section v-show="publishersExpanded" id="plugin-publishers" class="stack gap-3 border border-line rounded-surface bg-surface p-4" :aria-label="messages.plugins.publishers.title">
      <HelpText>
        {{ messages.plugins.publishers.detail }}
      </HelpText>

      <!--
        What this device knows about the keys that sign packages. `seen` means "a verified signature came from this key and
        nobody has decided yet"; the decision is the user's, and it is per key rather than per publisher name.
      -->
      <AEmpty v-if="publishers.length === 0" :description="messages.plugins.publishers.empty" />
      <AList v-else size="small">
        <AList.Item v-for="publisher in publishers" :key="publisher.publisherId" :style="{ paddingInline: 0 }">
          <div class="row-between gap-3">
            <span class="min-w-0 stack gap-0.5">
              <span class="truncate text-sm" :title="publisher.publisherId">{{ publisher.publisherId }}</span>
              <!--
                ★ Not truncated: [07 §104](../design-docs/07-plugins.md) says the fingerprint must be checkable — 换行 or
                copy — and explicitly rejects showing "不可核对的前后几个字符". A clipped fingerprint cannot be compared
                with the one a person was given elsewhere, which is the only thing a fingerprint is for.
              -->
              <span class="min-w-0 select-text break-all text-xs text-ink-dim font-mono" :title="publisher.fingerprint">{{ publisher.fingerprint }}</span>
            </span>
            <span class="row gap-2">
              <ATag class="m-0">
                {{ messages.plugins.publishers.trustStates[publisher.trust as keyof typeof messages.plugins.publishers.trustStates] ?? publisher.trust }}
              </ATag>
              <AButton
                v-if="publisher.trust !== 'trusted'"
                size="small"
                :loading="deciding === publisher.publisherId"
                @click="decidePublisher(publisher.publisherId, publisher.fingerprint, 'trusted')"
              >
                {{ messages.plugins.publishers.trust }}
              </AButton>
              <AButton
                v-if="publisher.trust !== 'revoked'"
                size="small"
                :loading="deciding === publisher.publisherId"
                @click="decidePublisher(publisher.publisherId, publisher.fingerprint, 'revoked')"
              >
                {{ messages.plugins.publishers.revoke }}
              </AButton>
            </span>
          </div>
          <!-- ★ Under the row it belongs to: `deciding` is per key, and one page-level slot made two refusals overwrite each other. -->
          <ErrorNotice
            v-if="publisherErrors[publisher.publisherId]"
            :code="publisherErrors[publisher.publisherId]!.code"

            :actions="publisherErrors[publisher.publisherId]!.actions"
          />
        </AList.Item>
      </AList>
    </section>

    <!--
      The install form. Two sources, named rather than guessed: a directory on this device, or a package id in the
      registry configured on the settings page. The mode is a control instead of a rule that inspects the text, because
      `@acme/hello` and `C:\plugins\hello` are both "a string with a slash" and a page that guessed would eventually guess
      wrong.
    -->
    <section v-show="installExpanded" id="plugin-install" class="stack gap-4 border border-line rounded-surface bg-surface p-4" :aria-label="messages.plugins.installHeading">
      <ASegmented v-model:value="installMode" :options="installOptions" :disabled="installing" data-testid="install-mode" />
      <div class="row flex-wrap gap-2">
        <AInput
          v-model:value="installPath"
          :placeholder="installLabel"
          :aria-label="installLabel"
          :disabled="installing"
          class="min-w-0 w-full sm:flex-1"
          allow-clear
          @press-enter="submitInstall"
        />
        <AButton data-testid="install-submit" :loading="installing || previewing !== undefined" :disabled="installPath.trim() === ''" @click="install">
          {{ installing ? messages.plugins.installing : messages.plugins.installAction }}
        </AButton>

        <!--
          ★ A directory path typed by hand is the one form here that can be got wrong without noticing. The picker answers
          the same string the input takes, so the install path itself is unchanged.
        -->
        <AButton v-if="installMode === 'path'" :loading="choosing" :disabled="installing" @click="chooseDirectory">
          {{ messages.plugins.chooseDirectory }}
        </AButton>
        <AButton v-if="installMode === 'path'" :loading="choosing" :disabled="installing" @click="choosePackage">
          {{ messages.plugins.choosePackage }}
        </AButton>
      </div>
    </section>

    <!--
      The registry's own index, which is now the whole "discover" view rather than a panel above the installed list
      ([07 §1](../../design-docs/ui-design/07-plugins.md): "registry search only in discover"). A result is a name and a
      description, not a decision: Preview opens the same permission dialog an install by id would, so a search hit
      cannot install anything the user has not seen.

      The panel stays mounted while another category is shown so a search's results are still there when the user comes
      back; `v-show` rather than `v-if` for that reason, and nothing here polls.
    -->
    <section v-show="pluginCategory === 'discover'" class="min-w-0 stack gap-3" data-testid="catalog-panel" :aria-label="messages.plugins.catalog.title">
      <div class="row gap-2">
        <AInput
          v-model:value="searchText"
          :placeholder="messages.plugins.catalog.placeholder"
          :aria-label="messages.plugins.catalog.placeholder"
          allow-clear
          @press-enter="submitSearch"
        />
        <ASelect v-model:value="searchSort" :options="sortOptions" class="w-fit" />
        <AButton :loading="searching" :disabled="searchText.trim() === '' || catalogBusy" @click="search">
          {{ messages.plugins.catalog.action }}
        </AButton>
      </div>
      <ErrorNotice v-if="catalogError" :code="catalogError.code" :actions="catalogError.actions" @retry="retryCatalog" />
      <AList v-if="results.length > 0" :data-source="results" size="small">
        <template #renderItem="{ item }">
          <AList.Item :style="{ paddingInline: 0 }">
            <div class="row-between gap-4">
              <div class="stack">
                <span class="truncate text-sm font-medium">
                  {{ item.title }} <span class="text-ink-dim">{{ item.version }}</span>
                </span>
                <span class="text-sm text-ink-dim">{{ item.description ?? item.pluginId }}</span>
              </div>
              <AButton size="small" :loading="previewing === item.pluginId" :disabled="previewing !== undefined || installing" @click="previewResult(item)">
                {{ messages.plugins.catalog.preview }}
              </AButton>
            </div>
          </AList.Item>
        </template>
      </AList>
      <AButton v-if="hasMore" :loading="loadingMore" :disabled="catalogBusy" block @click="loadMore">
        {{ messages.plugins.catalog.loadMore }}
      </AButton>
      <p v-else-if="searched && !catalogBusy && !catalogError && results.length === 0" class="m-0 text-sm text-ink-dim">
        {{ messages.plugins.catalog.empty }}
      </p>
      <!--
        Before the first search this view held nothing but the controls. The three states are separate on purpose
        ([07 §9](../../design-docs/ui-design/07-plugins.md)): never searched, searched with no match, and failed are not
        "no plugins", and only the first one can still offer the local install path.
      -->
      <p v-else-if="!searched && !catalogBusy && !catalogError" class="m-0 text-sm text-ink-dim">
        {{ messages.plugins.catalog.idle }}
      </p>
    </section>

    <!--
      A registry install asks first. The preview is the publisher's own claim — the signature is checked on the bytes —
      and it is a question, not an approval: the plugin still lands switched off and untrusted.
    -->
    <AModal
      :open="preview !== undefined"
      :title="messages.plugins.preview.title"
      :ok-text="messages.plugins.installAction"
      @ok="confirmPreview"
      @cancel="cancelPreview"
    >
      <div v-if="preview" class="stack gap-2">
        <p class="m-0 text-base font-medium">
          {{ preview.title }} · {{ preview.version }}
        </p>
        <HelpText>
          {{ messages.plugins.preview.publisher }}: {{ preview.publisherName ?? preview.publisher }}
        </HelpText>
        <p v-if="preview.publisherKey" class="m-0 text-sm text-ink-dim">
          {{ messages.plugins.preview.directoryKey }}: <span class="font-mono">{{ preview.publisherKey }}</span>
        </p>
        <p class="m-0 text-sm">
          {{ messages.plugins.preview.permissions }}
        </p>
        <ul v-if="preview.capabilities.length > 0" class="m-0 ps-5 text-sm">
          <li v-for="capability in preview.capabilities" :key="capability" class="font-mono">
            {{ capability }}
          </li>
        </ul>
        <p v-else class="m-0 text-sm text-ink-dim">
          {{ messages.plugins.preview.none }}
        </p>
        <HelpText>
          {{ preview.headless ? messages.plugins.preview.headless : messages.plugins.preview.ui }}
        </HelpText>
        <p v-if="preview.sizeBytes" class="m-0 text-sm text-ink-dim">
          {{ messages.plugins.preview.size.replace('{size}', formatBytes(preview.sizeBytes)) }}
        </p>
      </div>
    </AModal>

    <!-- Installing is not approving: the sentence says the plugin is off, because a user told only "installed" would
         look for a fault in a plugin that is behaving exactly as designed. -->
    <AAlert v-if="note" type="success" :message="note" closable @close="note = undefined" />

    <!--
      The question a signed package asks next, shown right after the install that produced it. It is deliberately not a
      modal: the plugin is off, nothing is running, and the fingerprint is something a person compares at their own pace.
    -->
    <AAlert v-if="pendingPublisher" type="info" show-icon :message="messages.plugins.publisherPrompt.title">
      <template #description>
        <div class="stack gap-2">
          <span class="text-sm">{{ messages.plugins.publisherPrompt.detail }}</span>
          <span class="break-all text-xs font-mono">{{ pendingPublisher.fingerprint }}</span>
          <span class="row gap-2">
            <AButton
              size="small"
              :loading="deciding === pendingPublisher.id"
              @click="trustPendingPublisher"
            >
              {{ messages.plugins.publisherPrompt.trust }}
            </AButton>
            <AButton
              size="small"
              :loading="deciding === pendingPublisher.id"
              @click="revokePendingPublisher"
            >
              {{ messages.plugins.publisherPrompt.revoke }}
            </AButton>
            <AButton size="small" @click="pendingPublisher = undefined">
              {{ messages.plugins.publisherPrompt.later }}
            </AButton>
          </span>
        </div>
      </template>
    </AAlert>

    <details v-if="removalLeftovers.length" class="text-xs text-ink-dim">
      <summary class="min-h-11 cursor-pointer py-3">
        {{ messages.plugins.leftBehind(removalLeftovers.length) }}
      </summary>
      <ul class="m-0 ps-5">
        <li v-for="path in removalLeftovers" :key="path" class="break-all font-mono">
          {{ path }}
        </li>
      </ul>
    </details>
    <ErrorNotice v-if="error" :code="error.code" :actions="error.actions" @retry="load" />

    <!--
      ★ The report the update command returns, kept until the user dismisses it. The shell also publishes a toast from the
      same report; a toast is gone in seconds, and "the package was not signed" is the kind of sentence a person wants to
      be able to re-read without updating again.
    -->
    <AAlert
      v-if="lastUpdate"
      type="success"
      show-icon
      closable
      :message="messages.plugins.updatedTo.replace('{pluginId}', lastUpdate.pluginId).replace('{version}', lastUpdate.version)"
      @close="lastUpdate = undefined"
    >
      <template v-if="lastUpdate.warnings.length > 0" #description>
        <p v-for="warning in lastUpdate.warnings" :key="warning" class="m-0">
          {{ warning }}
        </p>
      </template>
    </AAlert>

    <!--
      R28: an empty state is an invitation with a next step. The next step is now the **form above** — this screen can
      install — so the button here is the honest second option: read the list again, for a plugin installed from
      somewhere else. It said "this screen cannot install anything yet" while that was true; leaving the sentence behind
      would be the kind of comment that outlives its fact and sends the next reader looking for a missing feature.
    -->
    <!--
      The installed view: what this device has, whether it can run, and the publishers whose signatures it has seen.
      Hidden rather than unmounted when "discover" is shown, so filters, expanded management rows and the target plugin
      survive a round trip to the other category.
    -->
    <div v-show="pluginCategory === 'installed'" class="min-w-0 stack gap-6" data-testid="plugin-installed-view">
      <!-- Local filtering stays separate from the explicitly opened registry search. -->
      <div v-if="targetPluginId" class="min-w-0 row-between flex-wrap gap-2" data-testid="plugin-target">
        <span ref="targetHeading" tabindex="-1" class="min-w-0 break-all text-sm font-medium">{{ targetPlugin?.title ?? targetPluginId }}</span>
        <AButton size="small" @click="leaveTarget">
          {{ messages.plugins.showAll }}
        </AButton>
      </div>
      <AAlert v-if="targetPluginId && !targetPlugin && !loading && !error" type="info" :message="messages.plugins.notFound(targetPluginId)" />
      <AInput
        v-if="plugins.length > 0 && !targetPluginId"
        ref="filterInput"
        v-model:value="query"
        :placeholder="messages.plugins.filterPlaceholder"
        allow-clear
      />

      <AEmpty v-if="plugins?.length === 0 && !targetPluginId" :description="messages.plugins.empty">
        <template #image>
          <!--
          The icon rather than antd's default illustration: that illustration draws its own light-grey palette into a
          screen whose neutrals are all one hue (§1.4), and empty states are named as the places that must not compete
          for attention (R27).
        -->
          <span class="i-carbon-application text-xl text-ink-dim" aria-hidden="true" />
        </template>

        <AButton :loading="loading" @click="load">
          {{ messages.plugins.emptyAction }}
        </AButton>
      </AEmpty>

      <AEmpty v-else-if="visible.length === 0 && !targetPluginId" :description="messages.plugins.noMatch">
        <AButton size="small" @click="clearFilter">
          {{ messages.plugins.clearFilter }}
        </AButton>
      </AEmpty>

      <!--
      A plugin is a substantial object — a name, three machine facts, a description, two permission lists and possibly
      an update — so each one is a list item with several lines rather than a card (R26). The lines are labelled
      separately instead of joined by separators, which is the shape R21 retires.
    -->
      <AList v-else-if="visible.length > 0" :data-source="visible" row-key="pluginId">
        <template #renderItem="{ item: plugin }">
          <AList.Item :style="{ paddingInline: 0 }">
            <div class="min-w-0 w-full stack gap-3">
              <div class="min-w-0 row flex-wrap gap-3">
                <span class="min-w-0 w-full truncate text-base font-medium sm:w-auto sm:flex-1" :title="plugin.title">{{ plugin.title }}</span>

                <!-- R23: the state is the word in the tag; the colour is the redundancy that survives a greyscale shot. -->
                <!-- The switch is the user's decision about whether it runs; should_run is this **and** the consent. -->
                <ASwitch
                  :aria-label="messages.plugins.enableLabel(plugin.title)"
                  :checked="plugin.enabled"
                  :loading="operations.busy.get(plugin.pluginId) === 'toggle'"
                  :disabled="operations.busy.has(plugin.pluginId)"
                  @change="(checked) => toggle(plugin, checked as boolean)"
                />
                <ATag :color="stateColour(plugin.state)" class="shrink-0">
                  {{ messages.plugins.states[plugin.state as keyof typeof messages.plugins.states] ?? plugin.state }}
                </ATag>
                <!--
                  ★ Beside the state, not in the management panel: `07 §1` keeps a publisher that needs attention visible
                  on the object it belongs to after the publisher list moved into its own panel. Default rather than
                  warning colour — a revoked key is a decision, not a fault of the plugin, which the kernel keeps running.
                -->
                <ATag v-if="publisherNeedsAttention(plugin)" class="shrink-0" data-testid="plugin-publisher-untrusted">
                  {{ messages.plugins.publishers.untrusted }}
                </ATag>
                <AButton v-if="plugin.state === 'awaiting_consent'" size="small" @click="managementOpen[plugin.pluginId] = [plugin.pluginId]">
                  {{ messages.plugins.reviewPermissions }}
                </AButton>
                <!--
                ★ A faulted plugin is enabled and trusted; what failed was starting it. Retry asks again without
                changing either switch, which is the difference between "try it now" and "turn it off and on".
              -->
                <AButton
                  v-if="plugin.state === 'faulted'"
                  size="small"
                  :loading="operations.busy.get(plugin.pluginId) === 'reload'"
                  :disabled="operations.busy.has(plugin.pluginId)"
                  @click="retry(plugin)"
                >
                  {{ messages.plugins.retry }}
                </AButton>
                <AButton v-if="plugin.hasUi" :aria-label="messages.plugins.openLabel(plugin.title)" :loading="operations.busy.get(plugin.pluginId) === 'open'" :disabled="operations.busy.has(plugin.pluginId)" @click="open(plugin)">
                  {{ messages.plugins.openAction }}
                </AButton>
              </div>

              <!--
                ★ Beside the card, not on the page: a switch that refuses does not move, and the page-level notice is
                above a list the card may be many screens down. No `@retry`: `ErrorNotice` renders its buttons only when
                the refusal carries `actions`, and a switch refusal carries none — a handler here would never run.
              -->
              <ErrorNotice
                v-if="cardErrors[plugin.pluginId]"
                :code="cardErrors[plugin.pluginId]!.code"

                :actions="cardErrors[plugin.pluginId]!.actions"
              />

              <!--
              ★ The rung, when it is not `good`. A plugin that is over its limits is still `enabled` and may even be
              `running`; the state tag does not tell that story. Render one localized explanation of the reported rung;
              the quota's fixed English explanation repeats it. Detailed limits and fault reasons remain below.
            -->
              <AAlert
                v-if="plugin.health && plugin.health.health !== 'good'"
                :type="plugin.health.health === 'faulted' ? 'error' : 'warning'"
                :message="messages.plugins.healthLabel(plugin.health.health)"
                show-icon
              />

              <!--
              ★ Why it is not running, when the state word cannot carry the reason. Safe mode is the case that made this
              necessary: the host started without plugins after a crash loop, every card is `unavailable`, and the only
              sentence that tells the user what to fix is the host's own explanation. A page that guessed from `state`
              would tell them a plugin is broken when the host decided not to start any of them.
            -->
              <AAlert v-if="plugin.stateReason" type="warning" show-icon :message="plugin.stateReason" />

              <p v-if="plugin.description" class="m-0 text-sm text-ink-dim">
                {{ plugin.description }}
              </p>

              <AAlert
                v-if="plugin.update"
                :type="plugin.update.needsNewConsent ? 'warning' : 'info'"
                show-icon
                :message="messages.plugins.updateFound.replace('{version}', plugin.update.version)"
              >
                <template #description>
                  <span v-if="plugin.update.addedCapabilities.length === 0">
                    {{ messages.plugins.updateNoNewPermissions }}
                  </span>
                  <span v-else>
                    {{ messages.plugins.updateNewPermissions }} {{ plugin.update.addedCapabilities.join(', ') }}
                  </span>
                </template>
              </AAlert>

              <ACollapse :active-key="managementOpen[plugin.pluginId] ?? []" ghost data-plugin-management @change="keys => setManagement(plugin.pluginId, keys)">
                <ACollapse.Panel :key="plugin.pluginId" :header="messages.plugins.manage" force-render>
                  <div class="min-w-0 stack gap-4">
                    <dl class="m-0 row flex-wrap gap-4 text-xs">
                      <div class="row gap-1">
                        <dt class="m-0 text-ink-dim">
                          {{ messages.plugins.pluginId }}
                        </dt>
                        <dd class="m-0 break-all font-mono">
                          {{ plugin.pluginId }}
                        </dd>
                      </div>
                      <div class="row gap-1">
                        <dt class="m-0 text-ink-dim">
                          {{ messages.plugins.version }}
                        </dt>
                        <dd class="m-0 break-all font-mono">
                          {{ plugin.version }}
                        </dd>
                      </div>
                      <div class="row gap-1">
                        <dt class="m-0 text-ink-dim">
                          {{ messages.plugins.publisher }}
                        </dt>
                        <dd class="m-0 break-all font-mono">
                          {{ plugin.publisher }}
                        </dd>
                      </div>
                      <!-- ★ Four of the quota's numbers, when the report carries a quota. The rung and the sentence say how the
                         plugin is doing; these say what it is allowed to do, which is the other half of the same question. -->
                      <div v-if="healthLimits(plugin.health)" class="row gap-1">
                        <dt class="m-0 text-ink-dim">
                          {{ messages.plugins.limits }}
                        </dt>
                        <dd class="m-0">
                          {{ healthLimits(plugin.health) }}
                        </dd>
                      </div>
                    </dl>

                    <!-- ★ The rest of the quota, behind a disclosure. Four numbers answer "can it work"; the other nine answer
                       "what exactly did the host decide", and a user who needs them is already looking for this card. A native
                       `<details>` rather than a popover: it is the same disclosure with no portal to position, and a test can
                       read the content whether it is open or not. -->
                    <details v-if="quotaRows(plugin.health).length" class="text-xs">
                      <summary class="cursor-pointer text-ink-dim">
                        {{ messages.plugins.allLimits }}
                      </summary>
                      <dl class="m-0 mt-1 flex flex-wrap gap-x-4 gap-y-1">
                        <div v-for="row in quotaRows(plugin.health)" :key="row.label" class="row gap-1">
                          <dt class="m-0 text-ink-dim">
                            {{ row.label }}
                          </dt>
                          <dd class="m-0">
                            {{ row.value }}
                          </dd>
                        </div>
                      </dl>
                    </details>

                    <!--
                    What the registry publishes, when the user asked. It is not fetched on mount: one check is one request per
                    plugin, and a list of ten would make opening the page depend on ten round trips.
                  -->

                    <p v-if="plugin.updateCheckedAtMs !== undefined" class="m-0 text-xs text-ink-dim">
                      {{ messages.plugins.lastChecked.replace('{time}', formatDateTime(plugin.updateCheckedAtMs)) }}
                    </p>

                    <!--
                    What the plugin asked for, and what it was refused: the second list is the one that turns this screen from
                    a settings page into the thing a user reads before installing something. "Nothing" is a word rather than
                    an empty space, because an empty space reads as "not loaded".
                  -->
                    <div class="stack gap-1">
                      <div class="row flex-wrap gap-2">
                        <span class="text-xs text-ink-dim">{{ messages.plugins.granted }}</span>
                        <span v-if="plugin.granted?.length === 0" class="text-xs text-ink-dim">{{ messages.plugins.none }}</span>
                        <ATag v-for="capability in plugin.granted" :key="capability" class="max-w-full whitespace-normal break-all font-mono">
                          {{ capability }}
                        </ATag>
                      </div>

                      <div class="row flex-wrap gap-2">
                        <span class="text-xs text-ink-dim">{{ messages.plugins.refused }}</span>
                        <span v-if="plugin.denied?.length === 0" class="text-xs text-ink-dim">{{ messages.plugins.none }}</span>
                        <ATag v-for="capability in plugin.denied" :key="capability" color="error" class="max-w-full whitespace-normal break-all font-mono">
                          {{ capability }}
                        </ATag>
                      </div>
                    </div>

                    <!--
                    The decision itself: one checkbox per **requested** capability, starting from what was already granted.

                    ★ Not an "allow everything" button. This is the one screen where the user is asked what a plugin may do,
                    and a single button would hand over exactly what the per-capability model exists to itemise. `requested`
                    is what the manifest declares, so the boxes are what the *plugin* asks for rather than what it was given.
                  -->
                    <div v-if="plugin.requested?.length" class="stack gap-2">
                      <span class="text-xs text-ink-dim">{{ messages.plugins.requested }}</span>

                      <!--
                      `:value` + `@update:value` rather than `v-model`: the map may not have an entry yet on the first render,
                      and `v-model` on a possibly-undefined key does not type-check under `exactOptionalPropertyTypes`.
                    -->
                      <ACheckboxGroup
                        :value="selection[plugin.pluginId] ?? plugin.granted"
                        :disabled="operations.busy.has(plugin.pluginId)"
                        class="stack"
                        :style="{ alignSelf: 'flex-start' }"
                        @update:value="(picked) => (selection[plugin.pluginId] = picked as string[])"
                      >
                        <ACheckbox
                          v-for="capability in plugin.requested"
                          :key="capability"
                          :value="capability"
                          class="text-xs font-mono"
                        >
                          {{ capability }}
                        </ACheckbox>
                      </ACheckboxGroup>

                      <AButton
                        v-if="plugin.state === 'awaiting_consent' || permissionChanged(plugin)"
                        size="small"
                        class="self-start"
                        :loading="operations.busy.get(plugin.pluginId) === 'consent'"
                        :disabled="operations.busy.has(plugin.pluginId)"
                        @click="consent(plugin, selection[plugin.pluginId] ?? plugin.granted)"
                      >
                        {{ messages.plugins.consentAction }}
                      </AButton>
                    </div>

                    <!--
                    ★ What the plugin **actually did**, beside what it may do. The granted list is a permission; this is a record
                    of the gate's calls, which is what makes "granted" different from "used" (09 §159, 18 §5).

                    Reading is a separate question from granting, so the trail is *pulled* rather than pushed: a page that asked on
                    mount would read one ring per plugin that nobody asked about, and a page that read once would show activity from
                    before whatever the user came back to check.
                  -->
                    <PluginAudit :plugin-id="plugin.pluginId" :active="(managementOpen[plugin.pluginId] ?? []).includes(plugin.pluginId)" />

                    <!--
                    Removal asks first, and the question says what happens to the data: this is the one action on the card that
                    cannot be undone, and a button that only said "Remove" would leave the user to find out what it took with it.
                  -->
                    <div class="row flex-wrap gap-2 [&>.ant-modal-root]:contents">
                      <PluginSync :plugin="plugin" :operations="operations" />

                      <AButton size="small" :loading="operations.busy.get(plugin.pluginId) === 'check'" :disabled="operations.busy.has(plugin.pluginId)" @click="checkUpdate(plugin)">
                        {{ messages.plugins.checkUpdates }}
                      </AButton>

                      <AButton
                        v-if="plugin.update"
                        type="primary"
                        size="small"
                        :loading="operations.busy.get(plugin.pluginId) === 'update'"
                        :disabled="operations.busy.has(plugin.pluginId)"
                        @click="applyUpdate(plugin)"
                      >
                        {{ messages.plugins.updateAction }}
                      </AButton>

                      <AButton size="small" danger :disabled="operations.busy.has(plugin.pluginId)" @click="askRemove(plugin)">
                        {{ messages.plugins.removeAction }}
                      </AButton>
                    </div>
                  </div>
                </ACollapse.Panel>
              </ACollapse>
            </div>
          </AList.Item>
        </template>
      </AList>
    </div>
  </section>

  <!--
      ★ The removal dialog, not a one-line confirm, because there are two outcomes and one of them deletes user data.
      `get-container="false"` keeps it inside this component instead of teleporting to `body`: the page's own tests can
      then see the checkbox and the button, and a dialog that is testable is one whose decision is actually asserted.

      The checkbox starts unticked every time the dialog opens: "keep the data" is the reversible answer, and a checkbox
      that remembered the previous destructive choice would be a dialog that deletes on a second visit by default.
    -->
  <AModal
    :open="removing !== undefined"
    :title="messages.plugins.removeTitle"
    :get-container="false"
    :closable="!removalBusy"
    :mask-closable="!removalBusy"
    :keyboard="!removalBusy"
    @ok="remove()"
    @cancel="closeRemoval"
  >
    <template #footer>
      <AButton ref="removalCancel" :disabled="removalBusy" @click="closeRemoval">
        {{ messages.plugins.audit.cancel }}
      </AButton>
      <AButton type="primary" danger :loading="removalBusy" @click="remove">
        {{ messages.plugins.removeAction }}
      </AButton>
    </template>
    <div class="stack gap-3">
      <!--
        ★ No Retry action in here. This dialog already carries its own "Remove" in the footer (`@ok`), and a removal that
        failed is retried by pressing it again — a second Retry beside the first would be the same command under two
        names. The two `withoutRet` call sites above do the same for the same reason.
      -->
      <ErrorNotice v-if="removalError" :code="removalError.code" :actions="removalError.actions?.filter(action => action.kind !== 'retry')" @retry="remove" />
      <p class="m-0 text-sm">
        {{ messages.plugins.removeDetail(removing?.title ?? '') }}
      </p>
      <span class="break-all text-xs text-ink-dim font-mono">
        {{ messages.plugins.removeDataPath(removing?.dataPath ?? '') }}
      </span>
      <ACheckbox
        :checked="removeData"
        :disabled="removalBusy"
        @update:checked="(checked) => (removeData = checked)"
      >
        {{ messages.plugins.removeDeleteData }}
      </ACheckbox>
    </div>
  </AModal>
</template>

<script setup lang="ts">
import type { CatalogItem, InstallPreviewDto, InstallReport, PluginSnapshot, PublisherDto, SuggestedAction, UpdateInfo } from '@synra/protocol'
import type { Failure } from './plugins/actions'
import { Alert as AAlert, Button as AButton, Checkbox as ACheckbox, CheckboxGroup as ACheckboxGroup, Collapse as ACollapse, Empty as AEmpty, Input as AInput, List as AList, Modal as AModal, Segmented as ASegmented, Select as ASelect, Switch as ASwitch, Tag as ATag } from 'ant-design-vue'
import { storeToRefs } from 'pinia'
import { computed, nextTick, onMounted, onScopeDispose, reactive, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import ErrorNotice from '../components/ErrorNotice.vue'
/**
 * What is installed, and what each one has been allowed to do.
 *
 * # Why granted and denied are shown rather than only granted
 *
 * A permissions screen that lists what a plugin *may* do tells the user nothing about what it *has tried* to do. The
 * design document asks for both, and it is the denied column that turns the screen from a settings page into the thing a
 * user actually reads before installing something.
 *
 * # Why the lifecycle state is a field and not a colour
 *
 * The second plugin in the fake snapshot is `enabled` **and** `faulted`. A screen that derived its badge from `enabled`
 * would show that plugin as working, which is the specific mistake `PluginSnapshot.state` exists to prevent.
 */
import HelpText from '../components/HelpText.vue'
import PageHeader from '../components/PageHeader.vue'

import { formatBytes } from '../format'
import { kernel } from '../kernel'
import { formatDateTime } from '../locale-format'
import { messages } from '../messages'
import { usePluginsStore } from '../stores/plugins'
import { decide, installFrom, installFromRegistry, previewInstall, setEnabled, uninstall } from './plugins/actions'
import { useCatalog } from './plugins/catalog'
import { usePluginOperations } from './plugins/operations'
import PluginAudit from './plugins/PluginAudit.vue'
import PluginSync from './plugins/PluginSync.vue'

const pluginsStore = usePluginsStore()
const { plugins } = storeToRefs(pluginsStore)
const operations = usePluginOperations()

/** Which capabilities the user has ticked, per plugin. Seeded from `granted` when the cards arrive. */
const selection = reactive<Record<string, string[]>>({})

/**
 * The plugins as the list takes them.
 *
 * Copied because `AList`'s `dataSource` is typed as a mutable array and what the kernel answered is `readonly`. The
 * copy is made when the list changes rather than on every render.
 */
const installed = computed(() => [...plugins.value])

/** Exact editor values only; capability coverage and whether consent is needed remain kernel decisions. */
function sameDecision(left: readonly string[], right: readonly string[]): boolean {
  return left.length === right.length && left.every(capability => right.includes(capability))
}

function permissionChanged(plugin: PluginSnapshot): boolean {
  return !sameDecision(selection[plugin.pluginId] ?? plugin.granted, plugin.granted)
}

watch(plugins, (next, previous) => {
  const before = new Map((previous ?? []).map(plugin => [plugin.pluginId, plugin]))
  const current = new Set(next.map(plugin => plugin.pluginId))
  for (const plugin of next) {
    const draft = selection[plugin.pluginId]
    const old = before.get(plugin.pluginId)
    if (!draft || !old || sameDecision(draft, old.granted))
      selection[plugin.pluginId] = [...plugin.granted]
  }
  for (const id of Object.keys(selection)) {
    if (!current.has(id))
      delete selection[id]
  }
}, { immediate: true, flush: 'sync' })

/** The filter the user typed, matched against id, title and description. */
const query = ref('')

const route = useRoute()
const router = useRouter()
const targetPluginId = computed(() => typeof route.query.plugin === 'string' ? route.query.plugin.trim() : '')
const targetPlugin = computed(() => installed.value.find(plugin => plugin.pluginId === targetPluginId.value))
const targetHeading = ref<HTMLElement>()
const filterInput = ref<{ focus: () => void }>()
const installToggle = ref<{ $el: HTMLButtonElement }>()
const managementOpen = reactive<Record<string, string[]>>({})

function setManagement(id: string, keys: string | number | (string | number)[]): void {
  managementOpen[id] = (Array.isArray(keys) ? keys : [keys]).map(String)
}

// A notice selects an exact object, independently of the user's keyword filter. Query-only navigation reuses this page.
watch(targetPluginId, async (id, previous) => {
  if (previous)
    managementOpen[previous] = []
  if (!id)
    return
  managementOpen[id] = [id]
  await nextTick()
  targetHeading.value?.focus()
}, { immediate: true, flush: 'post' })

async function leaveTarget(): Promise<void> {
  const { plugin: _plugin, ...query } = route.query
  await router.replace({ path: route.path, query, hash: route.hash })
  await nextTick()
  if (filterInput.value)
    filterInput.value.focus()
  else
    installToggle.value?.$el.focus()
}

/** Drops only the locally typed filter. */
function clearFilter(): void {
  query.value = ''
}

/** What the list draws: the installed plugins the filter keeps. */
const visible = computed(() => {
  if (targetPluginId.value)
    return targetPlugin.value ? [targetPlugin.value] : []
  const needle = query.value.trim().toLowerCase()
  if (needle === '')
    return installed.value
  return installed.value.filter(plugin =>
    plugin.pluginId.toLowerCase().includes(needle)
    || plugin.title.toLowerCase().includes(needle)
    || (plugin.description ?? '').toLowerCase().includes(needle)
  )
})

/** The refusal itself, not its sentence: the surface needs the `code`, the message **key** and the `actions` (R33). */
const error = ref<{ code: string, message: string, actions?: readonly SuggestedAction[] | undefined } | undefined>(undefined)

/** Whether the list is being read, so the empty state's button can say that it is doing something. */
const loading = ref(false)

/**
 * The lifecycle state's tag colour.
 *
 * `enabled` is deliberately not consulted: a plugin that is enabled **and** faulted is exactly what
 * `PluginSnapshot.state` exists to report. The two states this milestone knows get a status colour and anything the
 * kernel invents later stays neutral, which is the honest default — the tag always carries the word (R23), so the
 * colour is redundancy rather than the state itself.
 */
function stateColour(state: string): 'success' | 'error' | 'warning' | 'default' {
  switch (state) {
    case 'active': return 'success'
    // ★ `faulted` is the plugin's own failure; `unreadable` is a manifest this build could not read, which is also a
    // fault the user cannot ignore. Both red.
    case 'faulted':
    case 'unreadable': return 'error'
    // ★ Warning, not default: both need the user to do something. `awaiting_consent` is switched on but not trusted —
    // the card looks running and is not — and `unavailable` is enabled and trusted but the shell could not start it.
    // Grey would put them beside `disabled`, which needs nothing.
    case 'awaiting_consent':
    case 'unavailable': return 'warning'
    default: return 'default'
  }
}

/** The publishers whose signatures this device has seen. */
const publishers = ref<readonly PublisherDto[]>([])

/**
 * Whether this plugin's publisher needs the user's attention.
 *
 * `PluginSnapshot` carries only the publisher id, so the answer comes from the publisher list by id — the same join an
 * install report already does. The two rungs are exactly the ones the entry's own counter uses
 * (`publishersNeedingDecision`): `seen` is a decision nobody has made, and `revoked` is a decision the kernel keeps
 * running anyway. **Leaving `revoked` out here would let the button say "1 needs a decision" while the card it belongs
 * to carries no mark at all** — the user would have to open the panel and match fingerprints to find which one.
 *
 * A publisher missing from the list yields `false`: the list is what this page read, and inventing a warning from an
 * absence would put a mark on every plugin during the first load.
 */
function publisherNeedsAttention(plugin: PluginSnapshot): boolean {
  return publishers.value.some(publisher => publisher.publisherId === plugin.publisher
    && (publisher.trust === 'seen' || publisher.trust === 'revoked'))
}

/** The publisher whose decision is being written. */
const deciding = ref<string | undefined>(undefined)

/** The signing key of the package just installed, when it has not been decided yet. */
const pendingPublisher = ref<{ id: string, fingerprint: string } | undefined>(undefined)

/** Trusts the key the install just showed, and closes the prompt. */
async function trustPendingPublisher(): Promise<void> {
  const pending = pendingPublisher.value
  if (!pending)
    return
  if (await decidePublisher(pending.id, pending.fingerprint, 'trusted') && pendingPublisher.value === pending)
    pendingPublisher.value = undefined
}

/** Revokes the key the install just showed — the opposite answer, from the same prompt. */
async function revokePendingPublisher(): Promise<void> {
  const pending = pendingPublisher.value
  if (!pending)
    return
  if (await decidePublisher(pending.id, pending.fingerprint, 'revoked') && pendingPublisher.value === pending)
    pendingPublisher.value = undefined
}

onMounted(load)

/** Reads the installed plugins and the publishers this device has met. */
async function load(): Promise<void> {
  loading.value = true
  error.value = undefined

  const [answer, publisherAnswer] = await Promise.all([
    kernel.call<readonly PluginSnapshot[]>('plugins_list'),
    kernel.call<readonly PublisherDto[]>('plugins_publishers')
  ])
  loading.value = false

  if (answer.ok) {
    pluginsStore.applySnapshot(answer.value)
  }
  else {
    error.value = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
  }
  if (!publisherAnswer.ok) {
    if (answer.ok) {
      error.value = {
        code: publisherAnswer.error.code,
        message: publisherAnswer.error.message,
        actions: publisherAnswer.error.actions
      }
    }
  }
  else if (Array.isArray(publisherAnswer.value)) {
    publishers.value = publisherAnswer.value
  }
}

/**
 * A refusal from an action taken **on one card**, kept beside that card.
 *
 * The page-level `error` sits above the list, and a card can be many screens below it — a switch that does not move
 * plus a sentence nobody can see reads as "the click did nothing". Keyed by plugin id so two cards failing at once
 * stay two facts, and so a second attempt on the same card clears only its own sentence.
 */
const cardErrors = reactive<Record<string, Failure | undefined>>({})

/**
 * The same rule for the publisher's own row, keyed by the key rather than by a plugin.
 *
 * `decidePublisher` was the last action still writing the page-level `error`, and the argument against it is narrower
 * here: the panel is near the top, so the sentence is not off screen. **What is still wrong is that one slot serves
 * every row** — two keys refused in a row, and the first sentence is gone before it is read.
 */
const publisherErrors = reactive<Record<string, Failure | undefined>>({})

/** Trusts or revokes a publisher's key, then re-reads so the row shows the decision the kernel stored. */
async function decidePublisher(publisherId: string, fingerprint: string, trust: 'trusted' | 'revoked'): Promise<boolean> {
  if (deciding.value !== undefined)
    return false
  deciding.value = publisherId
  delete publisherErrors[publisherId]
  const answer = await kernel.call<null>('plugins_publisher_decide', { publisherId, fingerprint, trust })
  deciding.value = undefined
  if (!answer.ok) {
    publisherErrors[publisherId] = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
    return false
  }
  await load()
  return true
}

/**
 * Records the ticked capabilities and replaces the card with the answer's own.
 *
 * The answer **is** the card, so the list is replaced rather than re-read: one call, and what the user sees is exactly
 * what the kernel just recorded — a re-read would answer the same question a moment later and about every plugin.
 */
async function consent(plugin: PluginSnapshot, granted: readonly string[]): Promise<void> {
  delete cardErrors[plugin.pluginId]
  const outcome = await operations.run(plugin.pluginId, 'consent', () => decide(kernel, plugin.pluginId, granted))
  if (!outcome)
    return
  if (!outcome.ok) {
    cardErrors[plugin.pluginId] = outcome.failure
    return
  }
  pluginsStore.upsert(outcome.value)
  selection[outcome.value.pluginId] = [...outcome.value.granted]
}

/**
 * Turns a plugin on or off.
 *
 * The list is **not** re-read here: the command publishes `PluginUpdated` and the page's own subscription reloads on
 * it. Replacing the card locally as well would be a second description of the same change, and the two would disagree
 * the first time the kernel answered something other than what the click assumed.
 */
async function toggle(plugin: PluginSnapshot, enabled: boolean): Promise<void> {
  delete cardErrors[plugin.pluginId]
  const outcome = await operations.run(plugin.pluginId, 'toggle', () => setEnabled(kernel, plugin.pluginId, enabled))
  if (!outcome)
    return
  if (!outcome.ok)
    cardErrors[plugin.pluginId] = outcome.failure
}

/**
 * What to tell the user after an action that reported rather than changed a card: an install, a sync or a removal.
 *
 * One ref rather than two, because the page shows one note under the form: two would fight for the same line, and a
 * message that replaced another without being read is a message that was never shown.
 */
const note = ref<string | undefined>(undefined)

/**
 * The plugin whose removal is being decided, and whether its data goes with it.
 *
 * Two refs beside the list rather than one object, because the dialog opens on one plugin and the checkbox is answered
 * after it opens: a single object would have to be replaced on every tick, and `removing` is what the template tests.
 */
const removing = ref<PluginSnapshot | undefined>(undefined)

/** Whether the removal being decided also deletes the plugin's data. Reset every time the dialog opens. */
const removeData = ref(false)
const removalError = ref<Failure>()
const removalCancel = ref<{ $el: HTMLButtonElement }>()
const removalBusy = computed(() => removing.value !== undefined && operations.busy.has(removing.value.pluginId))
const removalLeftovers = ref<readonly string[]>([])

function closeRemoval(): void {
  if (!removalBusy.value)
    removing.value = undefined
}

/** Opens the removal dialog for one plugin, with the reversible answer preselected. */
async function askRemove(plugin: PluginSnapshot): Promise<void> {
  if (operations.busy.has(plugin.pluginId) || removing.value)
    return
  removalError.value = undefined
  removeData.value = false
  removing.value = plugin
  await nextTick()
  removalCancel.value?.$el.focus()
}

/**
 * The quota a health report carries, as the few numbers worth showing.
 *
 * `usage` is the quota as JSON so the DTO does not freeze a field list that the quota layer owns; this reads the four a
 * user can act on and shows nothing when the report has none — an empty "Limits" row would be a label with no fact.
 */
function healthLimits(health: PluginSnapshot['health']): string {
  const parts: string[] = []
  const memory = quotaNumber(health, 'maxMemoryBytes')
  if (memory) {
    parts.push(`${formatBytes(memory)} ${messages.plugins.limitMemory}`)
  }
  const instances = quotaNumber(health, 'maxInstances')
  if (instances) {
    parts.push(`${instances} ${messages.plugins.limitInstances}`)
  }
  const calls = quotaNumber(health, 'hostCallsPerSec')
  if (calls) {
    parts.push(`${calls} ${messages.plugins.limitCalls}`)
  }
  const timeout = quotaNumber(health, 'callTimeoutMs')
  if (timeout) {
    parts.push(`${timeout / 1000} s ${messages.plugins.limitCallTime}`)
  }
  return parts.join(' · ')
}

/** One number out of the quota JSON, when it is there and is a number. */
function quotaNumber(health: PluginSnapshot['health'], key: string): number | undefined {
  if (health === undefined)
    return undefined
  const usage = health.usage
  if (typeof usage !== 'object' || usage === null)
    return undefined
  const value = (usage as Record<string, unknown>)[key]
  return typeof value === 'number' ? value : undefined
}

/** Every quota field the report carries, for the disclosure under the summary row. */
function quotaRows(health: PluginSnapshot['health']): { label: string, value: string }[] {
  const rows: { label: string, value: string }[] = []
  const add = (key: string, label: string, format: (value: number) => string = String): void => {
    const value = quotaNumber(health, key)
    if (value) {
      rows.push({ label, value: format(value) })
    }
  }
  add('maxMemoryBytes', messages.plugins.limitMemory, formatBytes)
  add('maxInstances', messages.plugins.limitInstances)
  add('maxStorageBytes', messages.plugins.limitStorage, formatBytes)
  add('maxOpenTransfers', messages.plugins.limitTransfers)
  add('hostCallsPerSec', messages.plugins.limitCalls)
  add('hostCallsBurst', messages.plugins.limitBurst)
  add('eventsPerSec', messages.plugins.limitEvents)
  add('networkRequestsPerSec', messages.plugins.limitNetwork)
  add('networkConcurrent', messages.plugins.limitConcurrent)
  add('deviceSendsPerSec', messages.plugins.limitSends)
  add('fsOpsPerSec', messages.plugins.limitFsOps)
  add('callFuel', messages.plugins.limitFuel)
  add('callTimeoutMs', messages.plugins.limitCallTime, value => `${value / 1000} s`)
  return rows
}

/**
 * Removes the plugin, with or without its data.
 *
 * The decision is the dialog's, passed through rather than defaulted at the call: `false` here and `false` in the
 * command's own default would be two places that can disagree with the sentence beside the button.
 */
async function remove(): Promise<void> {
  const plugin = removing.value
  if (plugin === undefined)
    return
  const deleteData = removeData.value
  removalError.value = undefined
  const outcome = await operations.run(plugin.pluginId, 'remove', () => uninstall(kernel, plugin.pluginId, deleteData))
  if (!outcome)
    return
  if (!outcome.ok) {
    removalError.value = outcome.failure
    return
  }
  // ★ No event for a removal — there is no card for a plugin that no longer exists — so the store is told directly, then
  // the list is read once. Forgetting here rather than only reloading means the card disappears even if the read fails.
  pluginsStore.forget(plugin.pluginId)
  removing.value = undefined
  const freed = formatBytes(outcome.value.bytesFreed)
  const kept = outcome.value.dataKept
    ? messages.plugins.removedKept(plugin.title, freed)
    : messages.plugins.removedDeleted(plugin.title, freed)
  // ★ The removal cannot fail on a locked file — the row is already gone — so this sentence is where the user learns that
  // something survived. The full paths are in the report; the page names the count it can act on. `leftovers` is optional
  // in the protocol (absent when empty), so the absence is the ordinary case.
  const leftovers = outcome.value.leftovers ?? []
  removalLeftovers.value = leftovers
  note.value = leftovers.length > 0
    ? `${kept} ${messages.plugins.leftBehind(leftovers.length)}`
    : kept
  await load()
}

/**
 * Opens the plugin's page in its own window.
 *
 * The window is the shell's to build (`plugin_ui_open`), not this page's: the label, the origin and the token are decided
 * before the WebView exists, and a page that assembled them would be a second place those rules live. This is why the
 * call answers `null` and the failure is shown here — the button is the request, not the window.
 */
async function open(plugin: PluginSnapshot): Promise<void> {
  delete cardErrors[plugin.pluginId]
  const answer = await operations.run(plugin.pluginId, 'open', () => kernel.call<null>('plugin_ui_open', { pluginId: plugin.pluginId }))
  if (!answer)
    return
  if (!answer.ok)
    cardErrors[plugin.pluginId] = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
}

/** The directory the user typed. */
const installPath = ref('')
const installExpanded = ref(false)

/** Where the install comes from: a directory this device has, or the registry it was configured with. */
const installMode = ref<'path' | 'registry'>('path')

/**
 * Which of the page's two questions is on screen: what is installed and can run, or what could be installed
 * ([07 §1](../../design-docs/ui-design/07-plugins.md)). Installed is the default because that is what a person opens the
 * page to check; the registry's search is not a thing to scroll past on the way to their own plugins.
 *
 * It is view state, not kernel state: switching does not re-read either list, and the search keeps whatever it has
 * already found.
 */
const pluginCategory = ref<'installed' | 'discover'>('installed')
const pluginCategoryOptions = computed(() => [
  { label: messages.plugins.categories.installed, value: 'installed' },
  { label: messages.plugins.categories.discover, value: 'discover' }
])

/** Whether the trust manager is open. View state only; it does not change what is read. */
const publishersExpanded = ref(false)

/**
 * Keys this device has seen but nobody has decided, plus keys the user revoked. Both are decisions waiting on a person:
 * a `seen` key asks whether to trust it, and a `revoked` key is one whose trust was withdrawn while the plugin that it
 * signed may still be running — the kernel does not stop a plugin when its key is revoked, it only refuses the next
 * install or update. `trusted` keys are settled and are not counted.
 */
const publishersNeedingDecision = computed(() => publishers.value.filter(publisher => publisher.trust === 'seen' || publisher.trust === 'revoked').length)

/** The mode's labels, as the segmented control shows them. */
const installLabel = computed(() => installMode.value === 'registry' ? messages.plugins.installName : messages.plugins.installPath)
const installOptions = computed(() => [
  { label: messages.plugins.installMode.path, value: 'path' },
  { label: messages.plugins.installMode.registry, value: 'registry' }
])

/** Whether an install is in flight, so the button says so and cannot be pressed twice. */
const installing = ref(false)

/** Whether the directory picker is open. */
const choosing = ref(false)

/**
 * Asks the platform for a directory and puts its path in the input.
 *
 * The dialog is loaded on demand, so a test or a build without the plugin does not fail before the user asks for it. A
 * cancelled picker leaves the field alone rather than clearing what the user typed.
 */
async function chooseDirectory(): Promise<void> {
  choosing.value = true
  error.value = undefined
  try {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const selected = await open({
      directory: true,
      multiple: false,
      title: messages.plugins.chooseDirectory
    })
    if (typeof selected === 'string')
      installPath.value = selected
  }
  catch {
    error.value = { code: 'internal', message: 'error.internal' }
  }
  finally {
    choosing.value = false
  }
}

/**
 * Asks the platform for a `.synra` package and puts its path in the input.
 *
 * The same input takes a directory or a package, and the install pipeline branches on which it received; this button only
 * decides which kind of picker opens. The filter is a hint, not a rule: the pipeline re-checks what the file is.
 */
async function choosePackage(): Promise<void> {
  choosing.value = true
  error.value = undefined
  try {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const selected = await open({
      directory: false,
      multiple: false,
      filters: [{ name: 'Synra package', extensions: ['synra'] }],
      title: messages.plugins.choosePackage
    })
    if (typeof selected === 'string')
      installPath.value = selected
  }
  catch {
    error.value = { code: 'internal', message: 'error.internal' }
  }
  finally {
    choosing.value = false
  }
}

/** The report of the last successful registry update, until the user dismisses it. */
const lastUpdate = ref<{ pluginId: string, version: string, warnings: readonly string[] } | undefined>(undefined)

/** Asks the registry about one plugin, and shows or forgets the last answer. */
async function checkUpdate(plugin: PluginSnapshot): Promise<void> {
  delete cardErrors[plugin.pluginId]
  const answer = await operations.run(plugin.pluginId, 'check', () => kernel.call<UpdateInfo | null>('plugins_check_update', {
    pluginId: plugin.pluginId
  }))
  if (!answer)
    return
  if (!answer.ok) {
    cardErrors[plugin.pluginId] = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
    return
  }
  const current = pluginsStore.plugin(plugin.pluginId)
  if (current) {
    const next = { ...current }
    if (answer.value)
      next.update = answer.value
    else
      delete next.update
    pluginsStore.upsert(next)
  }
}

/** Installs the newer version, then re-reads the list — the row the user sees comes from the store. */
async function applyUpdate(plugin: PluginSnapshot): Promise<void> {
  delete cardErrors[plugin.pluginId]
  const answer = await operations.run(plugin.pluginId, 'update', () => kernel.call<InstallReport>('plugins_update', { pluginId: plugin.pluginId }))
  if (!answer)
    return
  if (!answer.ok) {
    cardErrors[plugin.pluginId] = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
    return
  }
  lastUpdate.value = {
    pluginId: answer.value.pluginId,
    version: answer.value.version,
    warnings: answer.value.warnings ?? []
  }
  await load()
}

/**
 * Asks the kernel to load a faulted plugin again.
 *
 * The answer is the card, and the kernel publishes `pluginUpdated` beside it, so the store is what the page re-renders
 * from. A refusal leaves the card as the kernel described it rather than replacing it with a second sentence.
 */
async function retry(plugin: PluginSnapshot): Promise<void> {
  error.value = undefined
  const answer = await operations.run(plugin.pluginId, 'reload', () => kernel.call<PluginSnapshot>('plugins_reload', { pluginId: plugin.pluginId }))
  if (!answer)
    return
  if (!answer.ok)
    error.value = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
}

/** The package a registry install would fetch, shown before anything is downloaded. */
const preview = ref<InstallPreviewDto | undefined>(undefined)

const {
  text: searchText,
  sort: searchSort,
  results,
  searched,
  error: catalogError,
  hasMore,
  searching,
  loadingMore,
  busy: catalogBusy,
  search,
  loadMore,
  retry: retryCatalog
} = useCatalog(kernel)

const sortOptions = computed(() => [
  { label: messages.plugins.catalog.byRelevance, value: 'relevance' },
  { label: messages.plugins.catalog.byName, value: 'name' }
])

function submitSearch(event: KeyboardEvent): void {
  if (!event.isComposing && event.keyCode !== 229)
    void search()
}

function submitInstall(event: KeyboardEvent): void {
  if (!event.isComposing && event.keyCode !== 229)
    void install()
}

const previewing = ref<string>()
let previewRevision = 0

function cancelPreview(): void {
  previewRevision++
  preview.value = undefined
}

watch([installMode, installPath, installExpanded], cancelPreview, { flush: 'sync' })
onScopeDispose(cancelPreview)

/** One preview at a time, tied to the current draft and page lifetime. */
async function requestPreview(pluginId: string): Promise<void> {
  if (previewing.value !== undefined || installing.value)
    return
  const revision = ++previewRevision
  previewing.value = pluginId
  error.value = undefined
  const outcome = await previewInstall(kernel, pluginId)
  previewing.value = undefined
  if (revision !== previewRevision)
    return
  if (!outcome.ok) {
    error.value = outcome.failure
    return
  }
  preview.value = outcome.value
}

/** Opens the permission dialog for a search hit — the same dialog an install by id uses. */
async function previewResult(item: CatalogItem): Promise<void> {
  await requestPreview(item.pluginId)
}

/**
 * Whether a refusal is the kernel saying no registry is configured.
 *
 * Read from the sentence because that is all the kernel gives: `registry_install.rs` raises it as
 * `ErrorCode::Unsupported` with this wording, and `Unsupported` is shared with several other refusals, so the code
 * cannot tell them apart. Matching prose is fragile — but the alternative is a paragraph that names the problem and
 * leaves the user to find the settings page themselves, which is what `07 §5.1` asks us not to do.
 *
 * If the kernel ever gives this its own code, delete the string and switch on that.
 *
 * Nothing guards the kernel side of this pairing. The wording lives in `registry_install`, on a branch no test there can
 * reach (it needs `RegistryConfig` to resolve to zero mirrors, and the neighbouring offline test does not go through it),
 * so rewording it fails nothing and the button just disappears. The two tests in `plugin-page.mount.test.ts` guard only
 * this side; read them as "the interface reacts correctly to this sentence", not as "the sentence is stable".
 */
function isMissingRegistry(failure: { code: string, message: string }): boolean {
  return failure.message.includes('no registry is configured')
}

/** Installs from the source the user named, then re-reads the list. */
async function install(): Promise<void> {
  const from = installPath.value.trim()
  if (from === '' || installing.value || previewing.value !== undefined)
    return

  if (installMode.value === 'registry') {
    await requestPreview(from)
    return
  }
  installing.value = true
  error.value = undefined

  const outcome = await installFrom(kernel, from)
  installing.value = false
  await completeInstall(outcome.ok ? outcome.value : undefined, outcome.ok ? undefined : outcome.failure)
}

/** Installs what the preview described, after the user confirmed it. */
async function confirmPreview(): Promise<void> {
  const chosen = preview.value
  if (!chosen || installing.value)
    return
  preview.value = undefined
  installing.value = true
  error.value = undefined
  const outcome = await installFromRegistry(kernel, chosen.pluginId)
  installing.value = false
  await completeInstall(outcome.ok ? outcome.value : undefined, outcome.ok ? undefined : outcome.failure)
}

/** The steps both install sources share: the note, the list, and the publisher question. */
async function completeInstall(
  report: InstallReport | undefined,
  failure: Failure | undefined
): Promise<void> {
  if (!report) {
    if (failure) {
      error.value = isMissingRegistry(failure)
        ? { ...failure, actions: [{ kind: 'openSettings', section: 'plugins' } as const] }
        : failure
    }
    return
  }
  note.value = [messages.plugins.installedSwitchedOff, ...(report.warnings ?? [])].join(' ')
  installPath.value = ''
  installExpanded.value = false
  await nextTick()
  installToggle.value?.$el.focus()
  await load()

  // ★ Asked after the list is re-read, so a key the user already trusted (or a package with no signature) does not
  // produce a second prompt. The fingerprint is the one the install verified, not one parsed here from the package.
  const known = report.publisherId
    ? publishers.value.find(publisher => publisher.publisherId === report.publisherId)
    : undefined
  if (report.publisherId && report.fingerprint && known?.trust !== 'trusted')
    pendingPublisher.value = { id: report.publisherId, fingerprint: report.fingerprint }
}
</script>
