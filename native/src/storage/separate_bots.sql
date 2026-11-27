-- Runs in a single transaction. Keep entry IDs, sequence numbers and review tickets.
INSERT INTO bots SELECT id,data FROM chats WHERE json_extract(data,'$.kind')='bot';
INSERT INTO bot_requests SELECT r.key,r.chat FROM requests r JOIN bots b ON b.id=r.chat;

-- A second bot author means that the old bot-keyed timeline mixed participants.
-- Preserve it as a single read-only conversation rather than guessing who received each entry.
WITH histories AS (
    SELECT b.id,b.data,EXISTS(
        SELECT 1 FROM entries e WHERE e.chat=b.id AND json_extract(e.data,'$.kind')='send-message'
        AND COALESCE(json_extract(e.data,'$.author.id'),'') NOT IN (b.id,'user')
    ) AS mixed FROM bots b WHERE EXISTS (SELECT 1 FROM entries e WHERE e.chat=b.id)
)
INSERT INTO chats(id,data) SELECT 'history:'||id,
    json_set(data,'$.id','history:'||id,'$.kind','direct','$.bot',NULL,
        '$.includesUser',json('true'),'$.memberIds',json_array(id),
        '$.archived',json(CASE WHEN mixed THEN 'true' ELSE 'false' END),
        '$.directPair',CASE WHEN NOT mixed THEN 'dm:user:'||id END)
FROM histories;
UPDATE entries SET chat='history:'||chat WHERE chat IN (SELECT id FROM bots);

ALTER TABLE permission_reviews RENAME TO old_reviews;
CREATE TABLE permission_reviews(
    ticket TEXT PRIMARY KEY, chat TEXT NOT NULL, seq INTEGER NOT NULL, signature TEXT NOT NULL,
    target TEXT NOT NULL REFERENCES bots(id) ON DELETE CASCADE,
    reviewer TEXT NOT NULL REFERENCES bots(id) ON DELETE CASCADE, pane TEXT NOT NULL,
    UNIQUE(chat,seq), FOREIGN KEY(chat,seq) REFERENCES entries(chat,seq) ON DELETE CASCADE
);
INSERT INTO permission_reviews SELECT ticket,CASE WHEN chat IN (SELECT id FROM bots)
    THEN 'history:'||chat ELSE chat END,seq,signature,target,reviewer,pane FROM old_reviews;
DROP TABLE old_reviews;
DELETE FROM chats WHERE id IN (SELECT id FROM bots);
UPDATE bots SET data=json_set(data,'$.hasUnread',json('false'),'$.unreadCount',0,
    '$.lastMessageId',NULL,'$.lastMessagePreview',NULL,'$.lastReadSeq',0,'$.lastIncomingSeq',0);
PRAGMA user_version=1;
