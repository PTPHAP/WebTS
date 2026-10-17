export type NotificationCategory='messages'|'poke'|'friends'|'mail'|'activities'|'channel';
export type NotificationPolicy=Record<NotificationCategory,boolean>&{quiet:boolean};
export const defaultNotificationPolicy:NotificationPolicy={quiet:false,messages:true,poke:true,friends:true,mail:true,activities:true,channel:true};
export function shouldNotify(policy:NotificationPolicy,category:NotificationCategory){return !policy.quiet&&policy[category];}
export const notificationLabels:Partial<Record<NotificationCategory,string>>={messages:'聊天、私信与社区回复',poke:'戳一戳',friends:'好友申请与接受',mail:'站点信件',channel:'连接、频道和成员动态'};
