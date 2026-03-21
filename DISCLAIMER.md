# Legal Disclaimer

## Warranty Disclaimer

This software ("pagerunner") is provided "as is" without warranty of any kind, express or
implied, including but not limited to the warranties of merchantability, fitness for a
particular purpose, and non-infringement. In no event shall the authors or copyright holders
be liable for any claim, damages, or other liability, whether in an action of contract, tort,
or otherwise, arising from, out of, or in connection with the software or the use or other
dealings in the software.

## Limitation of Liability

To the maximum extent permitted by applicable law, the authors of Pagerunner shall not be
liable for any:

- Direct, indirect, incidental, special, consequential, or punitive damages
- Loss of data, revenue, profits, or business opportunities
- Actions taken by AI systems interpreting and executing browser automation commands
- Unintended or over-eager AI responses that submit forms, click buttons, or modify data
- Security breaches or unauthorized access resulting from user credential or profile configuration
- Costs of procurement of substitute services or technology
- Damages arising from reliance on AI-generated recommendations or actions

## Responsible Use

By using Pagerunner, you accept the following obligations:

**Website Terms of Service:** Many websites prohibit automated access in their own Terms of
Service, regardless of Chrome's ToS or any other policy. You are responsible for reviewing
and complying with the terms of service of any website you automate using Pagerunner. Pagerunner
provides the tool; compliance with destination-site rules is the user's responsibility.

**Data Protection (GDPR / CCPA):** When using Pagerunner in a business context to process
personal data belonging to other individuals, you act as a data controller under GDPR, CCPA,
and similar regulations. Pagerunner's anonymization feature (`anonymize: true`) implements
data minimization as a mitigation, but it is not a guarantee of regulatory compliance.
Ensuring your use of Pagerunner meets applicable data protection obligations is your
responsibility.

**Authorized Access Only:** Pagerunner must only be used to automate browser sessions you are
authorized to control. Using Pagerunner to access computer systems without authorization may
violate the Computer Fraud and Abuse Act (CFAA), the Computer Misuse Act, or equivalent laws
in your jurisdiction, and is also prohibited by Anthropic's Acceptable Use Policy.

## Data Flow & Privacy

When using Pagerunner with an AI assistant (Claude, OpenAI, etc.):

- Page content (text extracted via `get_content`), JavaScript evaluation results, and any
  data you pass to AI tools will be transmitted to and processed by the AI provider's servers,
  which may be located in various jurisdictions
- Screenshots captured via the `screenshot` tool contain the full rendered page, including any
  personal data visible on screen
- Using `anonymize: true` strips recognized PII categories before content reaches the AI, but
  no automated PII detection is complete — unusual formats, custom identifiers, or context-
  dependent sensitive data may not be detected
- Session snapshots (cookies, localStorage) are stored locally in an encrypted database
  (`~/.pagerunner/state.db`) and are never transmitted to any external service by Pagerunner itself

## Indemnification

You agree to indemnify and hold harmless the authors of Pagerunner from any claims, damages,
losses, or expenses (including reasonable legal fees) arising from:

- Your use or misuse of Pagerunner
- Your violation of any third-party Terms of Service or acceptable use policies
- Your non-compliance with applicable laws or regulations, including data protection laws
- Unauthorized access or security incidents related to your credential or profile configuration

## Updates

This disclaimer may be updated at any time. The current version is always available at
[DISCLAIMER.md](https://github.com/Enreign/pagerunner/blob/main/DISCLAIMER.md).
Continued use of Pagerunner constitutes acceptance of the current disclaimer.
