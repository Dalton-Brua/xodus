## Format followed by get_userstats Request / Response

POST https://userstats.xboxlive.com/batch HTTP/1.1
REQUIRED HEADERS:
x-xbl-contract-version: 2
authorization: XBL3.0

BODY: 
``{
"arrangebyfield":"xuid",
"xuids":[<xuid>,...],
"stats":[{"name":"MinutesPlayed","titleId":"<title_id>"},...]
}``

RESPONSE:
``{
"statlistscollection": [
        {
        "arrangebyfield":"xuid",
        "arrangebyfieldid":"<xuid>",
        "stats": [
            { 
                "titleid":"<title_id>",
                "name":"MinutesPlayed",
                "value":"<minutes_played>",
            }
        ]
        } 
    ]
    }``