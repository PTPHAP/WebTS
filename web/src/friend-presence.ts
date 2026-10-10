export type FriendLocation={connection:string;server:string;address:string;location:{server_name:string;server_uid:string;channel:number;channel_name:string;password:boolean}};
// A server-supplied location is a navigation hint, never permission or a password.
export function sameDestination(where:FriendLocation,serverUid?:string):boolean{return !!serverUid&&where.location.server_uid===serverUid;}
