export function normalizeConnectionErrorDetail(
    detailText,
    fallback = 'Connection lost. Click Connect to retry.'
) {
    return detailText || fallback;
}

export function applyConnectionStatusUi({
    statusKey,
    detailText = '',
    connectionStatusDiv,
    connectionStatusTitle,
    connectionStatusDetail,
    connectionStatusTitles,
    lastConnectionStatusKey,
    lastConnectionDetail,
    onStatusChange,
}) {
    if (!connectionStatusDiv || !connectionStatusTitle || !connectionStatusDetail) {
        return {
            lastConnectionStatusKey,
            lastConnectionDetail,
        };
    }

    if (statusKey === 'playing') {
        connectionStatusDiv.classList.add('hidden');
        if (typeof onStatusChange === 'function') {
            onStatusChange(statusKey, detailText);
        }
        return {
            lastConnectionStatusKey: statusKey,
            lastConnectionDetail: detailText,
        };
    }

    if (statusKey === lastConnectionStatusKey && detailText === lastConnectionDetail) {
        return {
            lastConnectionStatusKey,
            lastConnectionDetail,
        };
    }

    connectionStatusDiv.classList.remove('hidden');
    connectionStatusDiv.classList.remove(
        'connection-status--idle',
        'connection-status--connecting',
        'connection-status--negotiating',
        'connection-status--waiting',
        'connection-status--respawn',
        'connection-status--error'
    );

    const styleKey = statusKey === 'negotiating'
        ? 'connecting'
        : (statusKey === 'respawn' ? 'waiting' : statusKey);
    connectionStatusDiv.classList.add(`connection-status--${styleKey}`);
    connectionStatusTitle.textContent = connectionStatusTitles[statusKey] || 'Status';
    connectionStatusDetail.textContent = detailText;

    if (typeof onStatusChange === 'function') {
        onStatusChange(statusKey, detailText);
    }
    return {
        lastConnectionStatusKey: statusKey,
        lastConnectionDetail: detailText,
    };
}
